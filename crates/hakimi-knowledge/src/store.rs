use crate::graph::{EdgeType, KnowledgeGraph, NodeType};
use anyhow::Result;
use async_trait::async_trait;
use hakimi_common::KnowledgeSearcher;
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;

pub struct KnowledgeStore {
    path: PathBuf,
    graph: KnowledgeGraph,
    auto_save: bool,
}

impl KnowledgeStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            graph: KnowledgeGraph::new(),
            auto_save: true,
        }
    }

    /// Load the graph from the JSON file at `self.path`.
    pub fn load(&mut self) -> Result<()> {
        if self.path.exists() {
            let data = std::fs::read_to_string(&self.path)?;
            self.graph = KnowledgeGraph::from_json(&data)?;
        }
        Ok(())
    }

    /// Save the graph to the JSON file at `self.path`.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self.graph.to_json()?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn graph(&self) -> &KnowledgeGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut KnowledgeGraph {
        &mut self.graph
    }

    /// Add a node and auto-save if enabled.
    pub fn add_node(&mut self, node: NodeType) -> Result<()> {
        self.graph.add_node(node);
        if self.auto_save {
            self.save()?;
        }
        Ok(())
    }

    /// Add an edge and auto-save if enabled.
    pub fn add_edge(&mut self, from: &str, to: &str, edge: EdgeType) -> Result<()> {
        self.graph.add_edge(from, to, edge)?;
        if self.auto_save {
            self.save()?;
        }
        Ok(())
    }

    /// Remove a node and auto-save if enabled.
    pub fn remove_node(&mut self, key: &str) -> Result<()> {
        self.graph.remove_node(key);
        if self.auto_save {
            self.save()?;
        }
        Ok(())
    }

    /// Set auto-save behavior.
    pub fn set_auto_save(&mut self, auto_save: bool) {
        self.auto_save = auto_save;
    }

    /// Get auto-save status.
    pub fn auto_save(&self) -> bool {
        self.auto_save
    }
}

#[async_trait]
impl KnowledgeSearcher for KnowledgeStore {
    async fn search(&self, query: &str, limit: usize) -> hakimi_common::Result<JsonValue> {
        let nodes = self.graph.search(query);
        let results: Vec<JsonValue> = nodes
            .iter()
            .take(limit)
            .map(|n| {
                json!({
                    "key": n.key(),
                    "kind": n.kind(),
                    "preview": format!("{:?}", n)
                })
            })
            .collect();

        Ok(json!({
            "results": results,
            "count": results.len(),
            "query": query
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_store_persistence() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("knowledge.json");

        // Create and populate
        {
            let mut store = KnowledgeStore::new(path.clone());
            store.set_auto_save(false);
            store
                .graph_mut()
                .add_node(NodeType::Entity("alice".to_string()));
            store
                .graph_mut()
                .add_node(NodeType::Person("bob".to_string()));
            store
                .graph_mut()
                .add_edge("alice", "bob", EdgeType::Knows)
                .unwrap();
            store.save().unwrap();
        }

        // Reload and verify
        {
            let mut store = KnowledgeStore::new(path.clone());
            store.load().unwrap();
            assert_eq!(store.graph().node_count(), 2);
            assert_eq!(store.graph().edge_count(), 1);
            assert!(store.graph().has_node("alice"));
            assert!(store.graph().has_node("bob"));
        }
    }

    #[test]
    fn test_store_auto_save() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("auto_save.json");

        {
            let mut store = KnowledgeStore::new(path.clone());
            assert!(store.auto_save());

            store.add_node(NodeType::Entity("e1".to_string())).unwrap();
            store.add_node(NodeType::Entity("e2".to_string())).unwrap();
            store.add_edge("e1", "e2", EdgeType::RelatesTo).unwrap();
        }

        // Verify file was written
        assert!(path.exists());

        // Reload and verify
        let mut store2 = KnowledgeStore::new(path);
        store2.load().unwrap();
        assert_eq!(store2.graph().node_count(), 2);
        assert_eq!(store2.graph().edge_count(), 1);
    }

    #[test]
    fn test_store_remove_node_auto_save() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("remove.json");

        let mut store = KnowledgeStore::new(path.clone());
        store.add_node(NodeType::Entity("a".to_string())).unwrap();
        store.add_node(NodeType::Entity("b".to_string())).unwrap();
        store.add_edge("a", "b", EdgeType::RelatesTo).unwrap();

        store.remove_node("a").unwrap();
        assert_eq!(store.graph().node_count(), 1);
        assert_eq!(store.graph().edge_count(), 0);

        // Reload and verify
        let mut store2 = KnowledgeStore::new(path);
        store2.load().unwrap();
        assert_eq!(store2.graph().node_count(), 1);
        assert!(!store2.graph().has_node("a"));
    }
}
