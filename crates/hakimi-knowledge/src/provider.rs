use crate::graph::{EdgeType, KnowledgeGraph, NodeType};
use crate::store::KnowledgeStore;
use crate::vector_store::{VectorDocument, VectorStore};
use async_trait::async_trait;
use hakimi_common::{HakimiError, ToolDefinition};
use hakimi_transports::EmbeddingProvider;
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// A memory provider backed by a knowledge graph, with optional vector search.
pub struct KnowledgeProvider {
    store: Mutex<KnowledgeStore>,
    vector_store: Option<Mutex<VectorStore>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}

impl KnowledgeProvider {
    pub fn new(path: PathBuf) -> Self {
        let mut store = KnowledgeStore::new(path);
        let _ = store.load(); // Best-effort load
        Self {
            store: Mutex::new(store),
            vector_store: None,
            embedding_provider: None,
        }
    }

    pub fn with_vector_search(
        path: PathBuf,
        embedding_provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        let mut store = KnowledgeStore::new(path.clone());
        let _ = store.load(); // Best-effort load

        let vector_path = VectorStore::sidecar_path(&path);
        let mut vector_store = VectorStore::new(
            vector_path,
            embedding_provider.model_name(),
            embedding_provider.dimension(),
        );
        if let Err(e) = vector_store.load() {
            warn!(error = %e, "failed to load knowledge vector index; starting empty");
        }

        Self {
            store: Mutex::new(store),
            vector_store: Some(Mutex::new(vector_store)),
            embedding_provider: Some(embedding_provider),
        }
    }

    pub async fn graph_snapshot(&self) -> KnowledgeGraph {
        let store = self.store.lock().await;
        // Return a clone since we can't return references through Mutex
        // Use to_json/from_json for a deep copy
        let json = store.graph().to_json().unwrap_or_default();
        KnowledgeGraph::from_json(&json).unwrap_or_else(|_| KnowledgeGraph::new())
    }

    async fn embed_one(&self, text: &str) -> hakimi_common::Result<Option<Vec<f32>>> {
        let Some(provider) = &self.embedding_provider else {
            return Ok(None);
        };
        let inputs = vec![text.to_string()];
        let mut embeddings = provider.embed(&inputs).await?;
        Ok(embeddings.pop())
    }

    async fn upsert_vector_document(
        &self,
        id: String,
        kind: String,
        text: String,
        metadata: JsonValue,
    ) -> hakimi_common::Result<()> {
        let Some(embedding) = self.embed_one(&text).await? else {
            return Ok(());
        };
        let Some(vector_store) = &self.vector_store else {
            return Ok(());
        };
        let mut vectors = vector_store.lock().await;
        vectors
            .upsert(VectorDocument {
                id,
                kind,
                text,
                metadata,
                embedding,
            })
            .map_err(|e| HakimiError::ToolSimple(format!("vector upsert failed: {e}")))?;
        vectors
            .save()
            .map_err(|e| HakimiError::ToolSimple(format!("vector save failed: {e}")))?;
        Ok(())
    }

    async fn vector_search_json(
        &self,
        query: &str,
        limit: usize,
    ) -> hakimi_common::Result<Option<JsonValue>> {
        let Some(query_embedding) = self.embed_one(query).await? else {
            return Ok(None);
        };
        let Some(vector_store) = &self.vector_store else {
            return Ok(None);
        };
        let vectors = vector_store.lock().await;
        let results = vectors
            .search(&query_embedding, limit)
            .map_err(|e| HakimiError::ToolSimple(format!("vector search failed: {e}")))?;
        let count = results.len();
        Ok(Some(json!({
            "mode": "vector",
            "query": query,
            "count": count,
            "embedding_model": vectors.embedding_model(),
            "results": results,
        })))
    }

    fn entity_text(kind: &str, key: &str) -> String {
        format!("Knowledge {kind}: {key}")
    }

    fn relation_text(from: &str, relation: &str, to: &str) -> String {
        format!("Knowledge relation: {from} {relation} {to}")
    }

    fn edge_from_relation(relation: &str) -> EdgeType {
        EdgeType::from_relation(relation)
    }
}

#[async_trait]
impl hakimi_context::MemoryProvider for KnowledgeProvider {
    fn name(&self) -> &str {
        "knowledge-graph"
    }

    fn is_available(&self) -> bool {
        true // Always available; creates file on first save
    }

    fn system_prompt_block(&self) -> String {
        // Avoid blocking the async mutex from sync system prompt path. Knowledge
        // can still be reached through tools/prefetch; gateway already loads
        // file memory into prompt separately.
        String::new()
    }

    async fn prefetch(&self, query: &str) -> String {
        if let Ok(Some(value)) = self.vector_search_json(query, 5).await
            && let Some(results) = value.get("results").and_then(|v| v.as_array())
            && !results.is_empty()
        {
            let mut lines = vec!["Knowledge vector matches:".to_string()];
            for result in results {
                let score = result.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let kind = result
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("item");
                let text = result.get("text").and_then(|v| v.as_str()).unwrap_or("");
                lines.push(format!("- [{kind}] score={score:.3} {text}"));
            }
            return lines.join("\n");
        }

        let store = self.store.lock().await;
        let results = store.graph().search(query);
        if results.is_empty() {
            return String::new();
        }
        let mut lines = Vec::new();
        for node in results {
            lines.push(format!("[{}] {}", node.kind(), node.key()));
            // Include immediate neighbors for context
            let neighbors = store.graph().query_neighbors(node.key(), 1);
            for neighbor in neighbors {
                lines.push(format!("  -> {}", neighbor.key()));
            }
        }
        lines.join("\n")
    }

    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "knowledge_add_entity".to_string(),
                description: "Add an entity to the knowledge graph and vector index. Entities represent things, people, places, etc.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Unique identifier for the entity"
                        },
                        "kind": {
                            "type": "string",
                            "description": "Type of entity: entity, person, location, skill, tool, event, note, concept, fact, preference",
                            "enum": ["entity", "person", "location", "skill", "tool", "event", "note", "concept", "fact", "preference"]
                        }
                    },
                    "required": ["key", "kind"]
                }),
                toolset: "knowledge".to_string(),
            },
            ToolDefinition {
                name: "knowledge_add_relation".to_string(),
                description: "Add a relationship between two entities in the knowledge graph and vector index.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "from": {
                            "type": "string",
                            "description": "Key of the source entity"
                        },
                        "to": {
                            "type": "string",
                            "description": "Key of the target entity"
                        },
                        "relation": {
                            "type": "string",
                            "description": "Type of relationship: relates_to, depends_on, prefers, knows, part_of, caused_by, used_with, replaces, improves, has_property, temporal_before, or a custom string"
                        }
                    },
                    "required": ["from", "to", "relation"]
                }),
                toolset: "knowledge".to_string(),
            },
            ToolDefinition {
                name: "knowledge_search".to_string(),
                description: "Search the knowledge graph/vector index for entities matching a query.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query. Uses semantic vector search when embedding is configured, otherwise substring match"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 8)"
                        }
                    },
                    "required": ["query"]
                }),
                toolset: "knowledge".to_string(),
            },
            ToolDefinition {
                name: "knowledge_get_context".to_string(),
                description: "Get context around a specific entity (its neighbors).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Entity key"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "How many hops to traverse (default: 2)"
                        }
                    },
                    "required": ["key"]
                }),
                toolset: "knowledge".to_string(),
            },
            ToolDefinition {
                name: "knowledge_list".to_string(),
                description: "List all entities in the knowledge graph.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                toolset: "knowledge".to_string(),
            },
            ToolDefinition {
                name: "knowledge_stats".to_string(),
                description: "Get statistics about the knowledge graph and vector index.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                toolset: "knowledge".to_string(),
            },
        ]
    }

    async fn handle_tool_call(
        &self,
        name: &str,
        args: &JsonValue,
    ) -> hakimi_common::Result<String> {
        match name {
            "knowledge_add_entity" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'key' argument".into()))?;
                let kind = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'kind' argument".into()))?;

                let node = NodeType::from_kind_and_key(kind, key)
                    .ok_or_else(|| HakimiError::ToolSimple(format!("unknown kind: {kind}")))?;

                {
                    let mut store = self.store.lock().await;
                    store.graph_mut().add_node(node);
                    store
                        .save()
                        .map_err(|e| HakimiError::ToolSimple(format!("save failed: {e}")))?;
                }

                if let Err(e) = self
                    .upsert_vector_document(
                        format!("entity:{key}"),
                        kind.to_string(),
                        Self::entity_text(kind, key),
                        json!({"key": key, "kind": kind, "type": "entity"}),
                    )
                    .await
                {
                    warn!(error = %e, key = key, "failed to update entity vector index");
                }

                Ok(format!("Added {kind} '{key}' to knowledge graph"))
            }
            "knowledge_add_relation" => {
                let from = args
                    .get("from")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'from' argument".into()))?;
                let to = args
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'to' argument".into()))?;
                let relation = args
                    .get("relation")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'relation' argument".into()))?;

                {
                    let mut store = self.store.lock().await;
                    store
                        .graph_mut()
                        .add_edge(from, to, Self::edge_from_relation(relation))
                        .map_err(|e| HakimiError::ToolSimple(e.to_string()))?;
                    store
                        .save()
                        .map_err(|e| HakimiError::ToolSimple(format!("save failed: {e}")))?;
                }

                if let Err(e) = self
                    .upsert_vector_document(
                        format!("relation:{from}:{relation}:{to}"),
                        "relation".to_string(),
                        Self::relation_text(from, relation, to),
                        json!({"from": from, "to": to, "relation": relation, "type": "relation"}),
                    )
                    .await
                {
                    warn!(error = %e, from = from, to = to, relation = relation, "failed to update relation vector index");
                }

                Ok(format!(
                    "Added relation '{relation}' from '{from}' to '{to}'"
                ))
            }
            "knowledge_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'query' argument".into()))?;
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(8) as usize;

                if let Some(value) = self.vector_search_json(query, limit).await? {
                    let results = value
                        .get("results")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if !results.is_empty() {
                        let entries: Vec<String> = results
                            .iter()
                            .map(|r| {
                                let score = r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let kind = r.get("kind").and_then(|v| v.as_str()).unwrap_or("item");
                                let text = r.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                format!("[{kind}] score={score:.3} {text}")
                            })
                            .collect();
                        return Ok(entries.join("\n"));
                    }
                }

                let store = self.store.lock().await;
                let results = store.graph().search(query);
                if results.is_empty() {
                    Ok("No matching entities found.".to_string())
                } else {
                    let entries: Vec<String> = results
                        .iter()
                        .take(limit)
                        .map(|n| format!("[{}] {}", n.kind(), n.key()))
                        .collect();
                    Ok(entries.join("\n"))
                }
            }
            "knowledge_get_context" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::ToolSimple("missing 'key' argument".into()))?;
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;

                let store = self.store.lock().await;
                let neighbors = store.graph().query_neighbors(key, depth);
                if neighbors.is_empty() {
                    Ok(format!("No neighbors found for '{key}'"))
                } else {
                    let entries: Vec<String> = neighbors
                        .iter()
                        .map(|n| format!("[{}] {}", n.kind(), n.key()))
                        .collect();
                    Ok(format!("Neighbors of '{key}':\n{}", entries.join("\n")))
                }
            }
            "knowledge_list" => {
                let store = self.store.lock().await;
                let nodes = store.graph().all_nodes();
                if nodes.is_empty() {
                    Ok("Knowledge graph is empty.".to_string())
                } else {
                    let entries: Vec<String> = nodes
                        .iter()
                        .map(|n| format!("[{}] {}", n.kind(), n.key()))
                        .collect();
                    Ok(format!("Knowledge graph entities:\n{}", entries.join("\n")))
                }
            }
            "knowledge_stats" => {
                let store = self.store.lock().await;
                let stats = store.graph().stats();
                let vector_line = if let Some(vectors) = &self.vector_store {
                    let vectors = vectors.lock().await;
                    format!(
                        "\n- Vector documents: {}\n- Embedding model: {}\n- Embedding dimension: {}",
                        vectors.document_count(),
                        vectors.embedding_model(),
                        vectors.embedding_dimension()
                    )
                } else {
                    "\n- Vector search: disabled".to_string()
                };
                Ok(format!(
                    "Knowledge graph stats:\n- Nodes: {}\n- Edges: {}\n- Connected components: {}\n- Avg degree: {:.2}{}",
                    stats.node_count,
                    stats.edge_count,
                    stats.connected_components,
                    stats.avg_degree,
                    vector_line
                ))
            }
            other => Err(HakimiError::ToolSimple(format!(
                "Unknown knowledge tool: {other}"
            ))),
        }
    }
}

#[async_trait]
impl hakimi_common::KnowledgeSearcher for KnowledgeProvider {
    async fn search(&self, query: &str, limit: usize) -> hakimi_common::Result<JsonValue> {
        if let Some(value) = self.vector_search_json(query, limit).await? {
            let has_results = value
                .get("results")
                .and_then(|v| v.as_array())
                .map(|r| !r.is_empty())
                .unwrap_or(false);
            if has_results {
                debug!(query = query, "knowledge vector search hit");
                return Ok(value);
            }
        }

        let store = self.store.lock().await;
        store.search(query, limit).await
    }
}
