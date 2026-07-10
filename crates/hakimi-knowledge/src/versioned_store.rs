use crate::graph::KnowledgeGraph;
use crate::version::KnowledgeVersion;
use crate::version_store::VersionStore;
use anyhow::Result;
use std::path::PathBuf;

pub struct VersionedKnowledgeStore {
    graph_path: PathBuf,
    version_store: VersionStore,
    graph: KnowledgeGraph,
    auto_version: bool,
}

impl VersionedKnowledgeStore {
    /// Create a new versioned knowledge store
    pub fn new(graph_path: PathBuf, version_db_path: PathBuf) -> Result<Self> {
        let version_store = VersionStore::new(&version_db_path)?;

        Ok(Self {
            graph_path,
            version_store,
            graph: KnowledgeGraph::new(),
            auto_version: true,
        })
    }

    /// Load the graph from disk
    pub fn load(&mut self) -> Result<()> {
        if self.graph_path.exists() {
            let data = std::fs::read_to_string(&self.graph_path)?;
            self.graph = KnowledgeGraph::from_json(&data)?;
        }
        Ok(())
    }

    /// Save the graph to disk
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.graph_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self.graph.to_json()?;
        std::fs::write(&self.graph_path, &json)?;

        // Auto-create version if enabled
        if self.auto_version {
            self.create_version(None)?;
        }

        Ok(())
    }

    /// Create a new version snapshot of the current graph
    pub fn create_version(&self, change_summary: Option<String>) -> Result<()> {
        let json = self.graph.to_json()?;

        let version_number = self
            .version_store
            .get_latest_version_number("knowledge_graph")?
            .unwrap_or(0)
            + 1;

        let version = KnowledgeVersion::new(
            "knowledge_graph".to_string(),
            version_number,
            json,
            serde_json::json!({
                "node_count": self.graph.node_count(),
                "edge_count": self.graph.edge_count(),
            }),
            change_summary,
        );

        self.version_store.save_version(&version)?;
        Ok(())
    }

    /// Get all version history
    pub fn get_version_history(&self) -> Result<Vec<KnowledgeVersion>> {
        self.version_store.get_all_versions("knowledge_graph")
    }

    /// Rollback to a specific version
    pub fn rollback_to_version(&mut self, version: i32) -> Result<()> {
        let version_data = self
            .version_store
            .get_version("knowledge_graph", version)?
            .ok_or_else(|| anyhow::anyhow!("Version {} not found", version))?;

        // Restore graph from version
        self.graph = KnowledgeGraph::from_json(&version_data.content)?;

        // Save to disk
        self.save()?;

        Ok(())
    }

    /// Get difference between two versions
    pub fn diff_versions(&self, version1: i32, version2: i32) -> Result<String> {
        let v1 = self
            .version_store
            .get_version("knowledge_graph", version1)?
            .ok_or_else(|| anyhow::anyhow!("Version {} not found", version1))?;

        let v2 = self
            .version_store
            .get_version("knowledge_graph", version2)?
            .ok_or_else(|| anyhow::anyhow!("Version {} not found", version2))?;

        // Simple diff: compare JSON
        let v1_json: serde_json::Value = serde_json::from_str(&v1.content)?;
        let v2_json: serde_json::Value = serde_json::from_str(&v2.content)?;

        let diff_text = format!(
            "Version {} -> Version {}\n\nVersion {} metadata: {:?}\nVersion {} metadata: {:?}\n\nContent changed: {}",
            version1,
            version2,
            version1,
            v1.metadata,
            version2,
            v2.metadata,
            v1_json != v2_json
        );

        Ok(diff_text)
    }

    /// Access the graph
    pub fn graph(&self) -> &KnowledgeGraph {
        &self.graph
    }

    /// Mutably access the graph
    pub fn graph_mut(&mut self) -> &mut KnowledgeGraph {
        &mut self.graph
    }

    /// Enable/disable auto-versioning on save
    pub fn set_auto_version(&mut self, auto_version: bool) {
        self.auto_version = auto_version;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeType, NodeType};
    use tempfile::TempDir;

    #[test]
    fn test_versioned_store_basic() {
        let tmp = TempDir::new().unwrap();
        let graph_path = tmp.path().join("graph.json");
        let version_db = tmp.path().join("versions.db");

        let mut store = VersionedKnowledgeStore::new(graph_path, version_db).unwrap();

        // Add initial data
        store
            .graph_mut()
            .add_node(NodeType::Entity("alice".to_string()));
        store.save().unwrap();

        // Check version was created
        let versions = store.get_version_history().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, 1);
    }

    #[test]
    fn test_version_rollback() {
        let tmp = TempDir::new().unwrap();
        let graph_path = tmp.path().join("graph.json");
        let version_db = tmp.path().join("versions.db");

        let mut store = VersionedKnowledgeStore::new(graph_path, version_db).unwrap();

        // V1: Add alice
        store
            .graph_mut()
            .add_node(NodeType::Entity("alice".to_string()));
        store.save().unwrap();

        // V2: Add bob
        store
            .graph_mut()
            .add_node(NodeType::Person("bob".to_string()));
        store.save().unwrap();

        // V3: Add edge
        store
            .graph_mut()
            .add_edge("alice", "bob", EdgeType::Knows)
            .unwrap();
        store.save().unwrap();

        assert_eq!(store.graph().node_count(), 2);
        assert_eq!(store.graph().edge_count(), 1);

        // Rollback to V1
        store.rollback_to_version(1).unwrap();

        assert_eq!(store.graph().node_count(), 1);
        assert_eq!(store.graph().edge_count(), 0);
        assert!(store.graph().has_node("alice"));
        assert!(!store.graph().has_node("bob"));
    }

    #[test]
    fn test_version_history() {
        let tmp = TempDir::new().unwrap();
        let graph_path = tmp.path().join("graph.json");
        let version_db = tmp.path().join("versions.db");

        let mut store = VersionedKnowledgeStore::new(graph_path, version_db).unwrap();

        // Create multiple versions
        for i in 1..=5 {
            store
                .graph_mut()
                .add_node(NodeType::Entity(format!("entity{}", i)));
            store
                .create_version(Some(format!("Added entity{}", i)))
                .unwrap();
        }

        let versions = store.get_version_history().unwrap();
        assert_eq!(versions.len(), 5);

        // Verify descending order
        for (i, version) in versions.iter().enumerate() {
            assert_eq!(version.version, (5 - i) as i32);
        }
    }

    #[test]
    fn test_diff_versions() {
        let tmp = TempDir::new().unwrap();
        let graph_path = tmp.path().join("graph.json");
        let version_db = tmp.path().join("versions.db");

        let mut store = VersionedKnowledgeStore::new(graph_path, version_db).unwrap();

        // V1
        store
            .graph_mut()
            .add_node(NodeType::Entity("e1".to_string()));
        store.save().unwrap();

        // V2
        store
            .graph_mut()
            .add_node(NodeType::Entity("e2".to_string()));
        store.save().unwrap();

        let diff = store.diff_versions(1, 2).unwrap();
        assert!(diff.contains("Version 1 -> Version 2"));
    }
}
