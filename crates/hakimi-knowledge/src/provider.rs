use crate::graph::{EdgeType, KnowledgeGraph, NodeType};
use crate::store::KnowledgeStore;
use async_trait::async_trait;
use hakimi_common::{HakimiError, ToolDefinition};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use std::sync::Mutex;

/// A memory provider backed by a knowledge graph.
pub struct KnowledgeProvider {
    store: Mutex<KnowledgeStore>,
}

impl KnowledgeProvider {
    pub fn new(path: PathBuf) -> Self {
        let mut store = KnowledgeStore::new(path);
        let _ = store.load(); // Best-effort load
        Self {
            store: Mutex::new(store),
        }
    }

    pub fn graph_snapshot(&self) -> KnowledgeGraph {
        let store = self.store.lock().unwrap();
        // Return a clone since we can't return references through Mutex
        // Use to_json/from_json for a deep copy
        let json = store.graph().to_json().unwrap_or_default();
        KnowledgeGraph::from_json(&json).unwrap_or_else(|_| KnowledgeGraph::new())
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
        let store = self.store.lock().unwrap();
        let graph = store.graph();
        if graph.node_count() == 0 {
            return String::new();
        }

        let mut lines = vec!["Knowledge graph memory:".to_string()];
        for node in graph.all_nodes() {
            lines.push(format!("- [{}] {}", node.kind(), node.key()));
        }
        let edges = graph.all_edges();
        if !edges.is_empty() {
            lines.push(String::new());
            lines.push("Relationships:".to_string());
            for (from, to, edge) in &edges {
                let edge_str = match edge {
                    EdgeType::RelatesTo => "relates_to",
                    EdgeType::DependsOn => "depends_on",
                    EdgeType::Prefers => "prefers",
                    EdgeType::Knows => "knows",
                    EdgeType::PartOf => "part_of",
                    EdgeType::CausedBy => "caused_by",
                    EdgeType::UsedWith => "used_with",
                    EdgeType::Replaces => "replaces",
                    EdgeType::Improves => "improves",
                    EdgeType::HasProperty => "has_property",
                    EdgeType::TemporalBefore => "temporal_before",
                    EdgeType::Custom(s) => s,
                };
                lines.push(format!("  {} -> {} -> {}", from.key(), edge_str, to.key()));
            }
        }
        lines.join("\n")
    }

    async fn prefetch(&self, query: &str) -> String {
        let store = self.store.lock().unwrap();
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
                description: "Add an entity to the knowledge graph. Entities represent things, people, places, etc.".to_string(),
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
            },
            ToolDefinition {
                name: "knowledge_add_relation".to_string(),
                description: "Add a relationship between two entities in the knowledge graph.".to_string(),
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
            },
            ToolDefinition {
                name: "knowledge_search".to_string(),
                description: "Search the knowledge graph for entities matching a query.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (substring match)"
                        }
                    },
                    "required": ["query"]
                }),
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
            },
            ToolDefinition {
                name: "knowledge_list".to_string(),
                description: "List all entities in the knowledge graph.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "knowledge_stats".to_string(),
                description: "Get statistics about the knowledge graph.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        ]
    }

    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> hakimi_common::Result<String> {
        match name {
            "knowledge_add_entity" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'key' argument".into()))?;
                let kind = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'kind' argument".into()))?;

                let node = match kind {
                    "entity" => NodeType::Entity(key.to_string()),
                    "person" => NodeType::Person(key.to_string()),
                    "location" => NodeType::Location(key.to_string()),
                    "skill" => NodeType::Skill(key.to_string()),
                    "tool" => NodeType::Tool(key.to_string()),
                    "event" => NodeType::Event(key.to_string()),
                    "note" => NodeType::Note(key.to_string()),
                    "concept" => NodeType::Concept(key.to_string()),
                    "fact" => NodeType::Fact(key.to_string()),
                    "preference" => NodeType::Preference(key.to_string()),
                    other => return Err(HakimiError::Tool(format!("unknown kind: {other}"))),
                };

                let mut store = self.store.lock().unwrap();
                store.graph_mut().add_node(node);
                store
                    .save()
                    .map_err(|e| HakimiError::Tool(format!("save failed: {e}")))?;
                Ok(format!("Added {kind} '{key}' to knowledge graph"))
            }
            "knowledge_add_relation" => {
                let from = args
                    .get("from")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'from' argument".into()))?;
                let to = args
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'to' argument".into()))?;
                let relation = args
                    .get("relation")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'relation' argument".into()))?;

                let edge = match relation {
                    "relates_to" => EdgeType::RelatesTo,
                    "depends_on" => EdgeType::DependsOn,
                    "prefers" => EdgeType::Prefers,
                    "knows" => EdgeType::Knows,
                    "part_of" => EdgeType::PartOf,
                    "caused_by" => EdgeType::CausedBy,
                    "used_with" => EdgeType::UsedWith,
                    "replaces" => EdgeType::Replaces,
                    "improves" => EdgeType::Improves,
                    "has_property" => EdgeType::HasProperty,
                    "temporal_before" => EdgeType::TemporalBefore,
                    custom => EdgeType::Custom(custom.to_string()),
                };

                let mut store = self.store.lock().unwrap();
                store
                    .graph_mut()
                    .add_edge(from, to, edge)
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                store
                    .save()
                    .map_err(|e| HakimiError::Tool(format!("save failed: {e}")))?;
                Ok(format!("Added relation '{relation}' from '{from}' to '{to}'"))
            }
            "knowledge_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'query' argument".into()))?;

                let store = self.store.lock().unwrap();
                let results = store.graph().search(query);
                if results.is_empty() {
                    Ok("No matching entities found.".to_string())
                } else {
                    let entries: Vec<String> = results
                        .iter()
                        .map(|n| format!("[{}] {}", n.kind(), n.key()))
                        .collect();
                    Ok(entries.join("\n"))
                }
            }
            "knowledge_get_context" => {
                let key = args
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing 'key' argument".into()))?;
                let depth = args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(2) as usize;

                let store = self.store.lock().unwrap();
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
                let store = self.store.lock().unwrap();
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
                let store = self.store.lock().unwrap();
                let stats = store.graph().stats();
                Ok(format!(
                    "Knowledge graph stats:\n- Nodes: {}\n- Edges: {}\n- Connected components: {}\n- Avg degree: {:.2}",
                    stats.node_count, stats.edge_count, stats.connected_components, stats.avg_degree
                ))
            }
            other => Err(HakimiError::Tool(format!("Unknown knowledge tool: {other}"))),
        }
    }
}
