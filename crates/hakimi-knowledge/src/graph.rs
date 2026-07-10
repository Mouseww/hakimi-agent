use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeType {
    Entity(String),
    Concept(String),
    Fact(String),
    Preference(String),
    Person(String),
    Location(String),
    Skill(String),
    Tool(String),
    Event(String),
    Note(String),
}

impl NodeType {
    pub fn from_kind_and_key(kind: &str, key: impl Into<String>) -> Option<Self> {
        let key = key.into();
        match kind.trim().to_ascii_lowercase().as_str() {
            "entity" => Some(NodeType::Entity(key)),
            "concept" => Some(NodeType::Concept(key)),
            "fact" => Some(NodeType::Fact(key)),
            "preference" => Some(NodeType::Preference(key)),
            "person" => Some(NodeType::Person(key)),
            "location" => Some(NodeType::Location(key)),
            "skill" => Some(NodeType::Skill(key)),
            "tool" => Some(NodeType::Tool(key)),
            "event" => Some(NodeType::Event(key)),
            "note" => Some(NodeType::Note(key)),
            _ => None,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            NodeType::Entity(s)
            | NodeType::Concept(s)
            | NodeType::Fact(s)
            | NodeType::Preference(s)
            | NodeType::Person(s)
            | NodeType::Location(s)
            | NodeType::Skill(s)
            | NodeType::Tool(s)
            | NodeType::Event(s)
            | NodeType::Note(s) => s,
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            NodeType::Entity(_) => "entity",
            NodeType::Concept(_) => "concept",
            NodeType::Fact(_) => "fact",
            NodeType::Preference(_) => "preference",
            NodeType::Person(_) => "person",
            NodeType::Location(_) => "location",
            NodeType::Skill(_) => "skill",
            NodeType::Tool(_) => "tool",
            NodeType::Event(_) => "event",
            NodeType::Note(_) => "note",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeType {
    RelatesTo,
    DependsOn,
    Prefers,
    Knows,
    PartOf,
    CausedBy,
    UsedWith,
    Replaces,
    Improves,
    HasProperty,
    TemporalBefore,
    Custom(String),
}

impl EdgeType {
    pub fn from_relation(relation: &str) -> Self {
        let trimmed = relation.trim();
        match trimmed.to_ascii_lowercase().as_str() {
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
            _ => EdgeType::Custom(trimmed.to_string()),
        }
    }

    pub fn relation_name(&self) -> &str {
        match self {
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
            EdgeType::Custom(value) => value.as_str(),
        }
    }
}

/// Statistics about the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub connected_components: usize,
    pub avg_degree: f64,
}

/// Serializable representation of the graph for JSON round-tripping.
#[derive(Serialize, Deserialize)]
struct SerializableGraph {
    nodes: Vec<NodeType>,
    edges: Vec<(String, String, EdgeType)>,
}

pub struct KnowledgeGraph {
    graph: StableDiGraph<NodeType, EdgeType>,
    key_to_idx: HashMap<String, NodeIndex>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            key_to_idx: HashMap::new(),
        }
    }

    /// Add a node, deduplicating by key(). Returns the NodeIndex.
    pub fn add_node(&mut self, node: NodeType) -> NodeIndex {
        let key = node.key().to_string();
        if let Some(&idx) = self.key_to_idx.get(&key) {
            self.graph[idx] = node;
            return idx;
        }
        let idx = self.graph.add_node(node);
        self.key_to_idx.insert(key, idx);
        idx
    }

    /// Add a directed edge between two nodes identified by their keys.
    pub fn add_edge(&mut self, from_key: &str, to_key: &str, edge: EdgeType) -> anyhow::Result<()> {
        let from_idx = self
            .key_to_idx
            .get(from_key)
            .ok_or_else(|| anyhow::anyhow!("node '{}' not found", from_key))?;
        let to_idx = self
            .key_to_idx
            .get(to_key)
            .ok_or_else(|| anyhow::anyhow!("node '{}' not found", to_key))?;
        self.graph.add_edge(*from_idx, *to_idx, edge);
        Ok(())
    }

    /// Remove a node and all its connected edges.
    pub fn remove_node(&mut self, key: &str) {
        if let Some(idx) = self.key_to_idx.remove(key) {
            self.graph.remove_node(idx);
        }
    }

    pub fn get_node(&self, key: &str) -> Option<&NodeType> {
        self.key_to_idx
            .get(key)
            .and_then(|idx| self.graph.node_weight(*idx))
    }

    pub fn has_node(&self, key: &str) -> bool {
        self.key_to_idx.contains_key(key)
    }

    /// BFS neighbor query up to `max_depth` hops from the given node.
    pub fn query_neighbors(&self, key: &str, max_depth: usize) -> Vec<&NodeType> {
        let start_idx = match self.key_to_idx.get(key) {
            Some(&idx) => idx,
            None => return vec![],
        };

        let mut visited = HashSet::new();
        visited.insert(start_idx);
        let mut queue = VecDeque::new();
        queue.push_back((start_idx, 0usize));
        let mut result = Vec::new();

        while let Some((idx, depth)) = queue.pop_front() {
            if depth > 0 {
                if let Some(node) = self.graph.node_weight(idx) {
                    result.push(node);
                }
            }
            if depth < max_depth {
                // Check both outgoing and incoming edges for undirected-like traversal
                for neighbor in self
                    .graph
                    .neighbors_directed(idx, petgraph::Direction::Outgoing)
                    .chain(
                        self.graph
                            .neighbors_directed(idx, petgraph::Direction::Incoming),
                    )
                {
                    if visited.insert(neighbor) {
                        queue.push_back((neighbor, depth + 1));
                    }
                }
            }
        }

        result
    }

    /// Find shortest path between two nodes using BFS. Returns keys along the path.
    pub fn find_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let from_idx = *self.key_to_idx.get(from)?;
        let to_idx = *self.key_to_idx.get(to)?;

        if from_idx == to_idx {
            return Some(vec![from.to_string()]);
        }

        let mut visited = HashSet::new();
        visited.insert(from_idx);
        let mut parent: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut queue = VecDeque::new();
        queue.push_back(from_idx);

        while let Some(current) = queue.pop_front() {
            // Search both directions for pathfinding
            for neighbor in self
                .graph
                .neighbors_directed(current, petgraph::Direction::Outgoing)
                .chain(
                    self.graph
                        .neighbors_directed(current, petgraph::Direction::Incoming),
                )
            {
                if visited.contains(&neighbor) {
                    continue;
                }
                visited.insert(neighbor);
                parent.insert(neighbor, current);

                if neighbor == to_idx {
                    // Reconstruct path
                    let mut path = Vec::new();
                    let mut node = to_idx;
                    path.push(self.graph[node].key().to_string());
                    while let Some(&p) = parent.get(&node) {
                        path.push(self.graph[p].key().to_string());
                        node = p;
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(neighbor);
            }
        }

        None
    }

    /// Extract a subgraph around the given query keys up to `depth` hops.
    pub fn get_context_subgraph(&self, query_keys: &[String], depth: usize) -> KnowledgeGraph {
        let mut included_keys = HashSet::new();

        for key in query_keys {
            included_keys.insert(key.clone());
            let neighbors = self.query_neighbors(key, depth);
            for n in neighbors {
                included_keys.insert(n.key().to_string());
            }
        }

        let mut sub = KnowledgeGraph::new();
        for key in &included_keys {
            if let Some(node) = self.get_node(key) {
                sub.add_node(node.clone());
            }
        }

        // Add edges between included nodes
        for edge_ref in self.graph.edge_indices() {
            if let Some((src, tgt)) = self.graph.edge_endpoints(edge_ref) {
                let src_key = self.graph[src].key();
                let tgt_key = self.graph[tgt].key();
                if included_keys.contains(src_key) && included_keys.contains(tgt_key) {
                    let edge = self.graph[edge_ref].clone();
                    let _ = sub.add_edge(src_key, tgt_key, edge);
                }
            }
        }

        sub
    }

    pub fn all_nodes(&self) -> Vec<&NodeType> {
        self.graph.node_weights().collect()
    }

    pub fn all_edges(&self) -> Vec<(&NodeType, &NodeType, &EdgeType)> {
        self.graph
            .edge_indices()
            .filter_map(|e| {
                let (src, tgt) = self.graph.edge_endpoints(e)?;
                Some((
                    self.graph.node_weight(src)?,
                    self.graph.node_weight(tgt)?,
                    self.graph.edge_weight(e)?,
                ))
            })
            .collect()
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Serialize the graph to JSON.
    pub fn to_json(&self) -> anyhow::Result<String> {
        let nodes: Vec<NodeType> = self.graph.node_weights().cloned().collect();
        let edges: Vec<(String, String, EdgeType)> = self
            .graph
            .edge_indices()
            .filter_map(|e| {
                let (src, tgt) = self.graph.edge_endpoints(e)?;
                Some((
                    self.graph.node_weight(src)?.key().to_string(),
                    self.graph.node_weight(tgt)?.key().to_string(),
                    self.graph.edge_weight(e)?.clone(),
                ))
            })
            .collect();

        let ser = SerializableGraph { nodes, edges };
        Ok(serde_json::to_string(&ser)?)
    }

    /// Deserialize a graph from JSON.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let ser: SerializableGraph = serde_json::from_str(json)?;
        let mut kg = KnowledgeGraph::new();
        for node in ser.nodes {
            kg.add_node(node);
        }
        for (from, to, edge) in ser.edges {
            // Silently skip edges referencing missing nodes
            let _ = kg.add_edge(&from, &to, edge);
        }
        Ok(kg)
    }

    /// Merge another graph into this one. Nodes with the same key are updated; edges are added.
    pub fn merge(&mut self, other: &KnowledgeGraph) {
        for node in other.all_nodes() {
            self.add_node(node.clone());
        }
        for edge_ref in other.graph.edge_indices() {
            if let Some((src, tgt)) = other.graph.edge_endpoints(edge_ref) {
                let src_key = other.graph[src].key();
                let tgt_key = other.graph[tgt].key();
                let edge = other.graph[edge_ref].clone();
                let _ = self.add_edge(src_key, tgt_key, edge);
            }
        }
    }

    /// Fuzzy substring search on node keys (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&NodeType> {
        let query_lower = query.to_lowercase();
        self.graph
            .node_weights()
            .filter(|n| n.key().to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Advanced search with scoring and options
    pub fn search_advanced(
        &self,
        query: &str,
        options: &crate::search::SearchOptions,
    ) -> Vec<crate::search::SearchResult> {
        use crate::search::SearchEngine;
        let nodes: Vec<&NodeType> = self.graph.node_weights().collect();
        SearchEngine::search(&nodes, query, options)
    }

    /// Search using TF-IDF scoring
    pub fn search_tfidf(
        &self,
        query: &str,
        options: &crate::search::SearchOptions,
    ) -> Vec<crate::search::SearchResult> {
        use crate::search::SearchIndex;
        let nodes: Vec<&NodeType> = self.graph.node_weights().collect();
        let mut index = SearchIndex::new();
        index.build(&nodes);
        index.search_tfidf(&nodes, query, options)
    }

    /// Compute graph statistics.
    pub fn stats(&self) -> GraphStats {
        let node_count = self.graph.node_count();
        let edge_count = self.graph.edge_count();
        let avg_degree = if node_count > 0 {
            (edge_count as f64 * 2.0) / node_count as f64
        } else {
            0.0
        };

        // Compute connected components using BFS on undirected interpretation
        let mut visited = HashSet::new();
        let mut components = 0usize;
        for node_idx in self.graph.node_indices() {
            if visited.contains(&node_idx) {
                continue;
            }
            components += 1;
            let mut queue = VecDeque::new();
            queue.push_back(node_idx);
            visited.insert(node_idx);
            while let Some(current) = queue.pop_front() {
                for neighbor in self
                    .graph
                    .neighbors_directed(current, petgraph::Direction::Outgoing)
                    .chain(
                        self.graph
                            .neighbors_directed(current, petgraph::Direction::Incoming),
                    )
                {
                    if visited.insert(neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        GraphStats {
            node_count,
            edge_count,
            connected_components: components,
            avg_degree,
        }
    }
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node() {
        let mut kg = KnowledgeGraph::new();
        let _idx = kg.add_node(NodeType::Entity("test_entity".to_string()));
        assert_eq!(kg.node_count(), 1);
        assert!(kg.has_node("test_entity"));
    }

    #[test]
    fn test_add_duplicate_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("dup".to_string()));
        kg.add_node(NodeType::Concept("dup".to_string()));
        // Same key: should overwrite but not duplicate
        assert_eq!(kg.node_count(), 1);
        assert_eq!(kg.get_node("dup").unwrap().kind(), "concept");
    }

    #[test]
    fn test_add_edge() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        assert_eq!(kg.edge_count(), 1);
    }

    #[test]
    fn test_add_edge_missing_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        let result = kg.add_edge("a", "missing", EdgeType::RelatesTo);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.remove_node("a");
        assert_eq!(kg.node_count(), 1);
        assert_eq!(kg.edge_count(), 0);
        assert!(!kg.has_node("a"));
    }

    #[test]
    fn test_get_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Person("alice".to_string()));
        let node = kg.get_node("alice").unwrap();
        assert_eq!(node.kind(), "person");
        assert_eq!(node.key(), "alice");
        assert!(kg.get_node("bob").is_none());
    }

    #[test]
    fn test_has_node() {
        let mut kg = KnowledgeGraph::new();
        assert!(!kg.has_node("x"));
        kg.add_node(NodeType::Entity("x".to_string()));
        assert!(kg.has_node("x"));
    }

    #[test]
    fn test_query_neighbors_depth_1() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("center".to_string()));
        kg.add_node(NodeType::Entity("n1".to_string()));
        kg.add_node(NodeType::Entity("n2".to_string()));
        kg.add_edge("center", "n1", EdgeType::RelatesTo).unwrap();
        kg.add_edge("center", "n2", EdgeType::DependsOn).unwrap();

        let neighbors = kg.query_neighbors("center", 1);
        assert_eq!(neighbors.len(), 2);
        let keys: HashSet<&str> = neighbors.iter().map(|n| n.key()).collect();
        assert!(keys.contains("n1"));
        assert!(keys.contains("n2"));
    }

    #[test]
    fn test_query_neighbors_depth_2() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.add_edge("b", "c", EdgeType::DependsOn).unwrap();

        let neighbors = kg.query_neighbors("a", 2);
        let keys: HashSet<&str> = neighbors.iter().map(|n| n.key()).collect();
        assert!(keys.contains("b"));
        assert!(keys.contains("c"));
    }

    #[test]
    fn test_find_path_direct() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();

        let path = kg.find_path("a", "b").unwrap();
        assert_eq!(path, vec!["a", "b"]);
    }

    #[test]
    fn test_find_path_indirect() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.add_edge("b", "c", EdgeType::DependsOn).unwrap();

        let path = kg.find_path("a", "c").unwrap();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_no_path() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        // No edge between them
        assert!(kg.find_path("a", "b").is_none());
    }

    #[test]
    fn test_context_subgraph() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_node(NodeType::Entity("d".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.add_edge("b", "c", EdgeType::DependsOn).unwrap();
        // d is isolated

        let sub = kg.get_context_subgraph(&["a".to_string()], 1);
        assert!(sub.has_node("a"));
        assert!(sub.has_node("b"));
        assert!(!sub.has_node("d"));
    }

    #[test]
    fn test_merge_graphs() {
        let mut kg1 = KnowledgeGraph::new();
        kg1.add_node(NodeType::Entity("a".to_string()));
        kg1.add_node(NodeType::Entity("b".to_string()));
        kg1.add_edge("a", "b", EdgeType::RelatesTo).unwrap();

        let mut kg2 = KnowledgeGraph::new();
        kg2.add_node(NodeType::Entity("c".to_string()));
        kg2.add_node(NodeType::Entity("b".to_string())); // duplicate key
        kg2.add_edge("b", "c", EdgeType::DependsOn).unwrap();

        kg1.merge(&kg2);
        assert_eq!(kg1.node_count(), 3); // a, b, c
        assert_eq!(kg1.edge_count(), 2); // a->b, b->c
    }

    #[test]
    fn test_search() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("alice_smith".to_string()));
        kg.add_node(NodeType::Person("bob_jones".to_string()));
        kg.add_node(NodeType::Entity("alice_wonderland".to_string()));

        let results = kg.search("alice");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("MyEntity".to_string()));

        let results = kg.search("myentity");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Person("bob".to_string()));
        kg.add_node(NodeType::Skill("rust".to_string()));
        kg.add_edge("a", "bob", EdgeType::Knows).unwrap();
        kg.add_edge("bob", "rust", EdgeType::Prefers).unwrap();

        let json = kg.to_json().unwrap();
        let kg2 = KnowledgeGraph::from_json(&json).unwrap();

        assert_eq!(kg2.node_count(), 3);
        assert_eq!(kg2.edge_count(), 2);
        assert!(kg2.has_node("a"));
        assert!(kg2.has_node("bob"));
        assert!(kg2.has_node("rust"));
        assert_eq!(kg2.get_node("a").unwrap().kind(), "entity");
        assert_eq!(kg2.get_node("bob").unwrap().kind(), "person");
    }

    #[test]
    fn test_empty_graph() {
        let kg = KnowledgeGraph::new();
        assert_eq!(kg.node_count(), 0);
        assert_eq!(kg.edge_count(), 0);
        assert!(kg.all_nodes().is_empty());
        assert!(kg.all_edges().is_empty());
        assert!(kg.search("anything").is_empty());
        assert!(kg.get_node("x").is_none());
        assert!(!kg.has_node("x"));

        let json = kg.to_json().unwrap();
        let kg2 = KnowledgeGraph::from_json(&json).unwrap();
        assert_eq!(kg2.node_count(), 0);
    }

    #[test]
    fn test_complex_graph_10_nodes() {
        let mut kg = KnowledgeGraph::new();
        for i in 0..10 {
            kg.add_node(NodeType::Entity(format!("node_{}", i)));
        }
        // Chain: 0->1->2->...->9
        for i in 0..9 {
            kg.add_edge(
                &format!("node_{}", i),
                &format!("node_{}", i + 1),
                EdgeType::RelatesTo,
            )
            .unwrap();
        }
        // Cross edge: 0->5
        kg.add_edge("node_0", "node_5", EdgeType::DependsOn)
            .unwrap();

        assert_eq!(kg.node_count(), 10);
        assert_eq!(kg.edge_count(), 10);

        let path = kg.find_path("node_0", "node_9").unwrap();
        assert_eq!(path.first().unwrap(), "node_0");
        assert_eq!(path.last().unwrap(), "node_9");

        let stats = kg.stats();
        assert_eq!(stats.node_count, 10);
        assert_eq!(stats.edge_count, 10);
        assert_eq!(stats.connected_components, 1);
    }

    #[test]
    fn test_all_nodes() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Concept("b".to_string()));
        let nodes = kg.all_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_all_edges() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.add_edge("b", "c", EdgeType::DependsOn).unwrap();

        let edges = kg.all_edges();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_node_types() {
        let mut kg = KnowledgeGraph::new();
        let types = vec![
            NodeType::Entity("e".into()),
            NodeType::Concept("c".into()),
            NodeType::Fact("f".into()),
            NodeType::Preference("p".into()),
            NodeType::Person("pe".into()),
            NodeType::Location("l".into()),
            NodeType::Skill("s".into()),
            NodeType::Tool("t".into()),
            NodeType::Event("ev".into()),
            NodeType::Note("n".into()),
        ];
        for nt in &types {
            kg.add_node(nt.clone());
        }
        assert_eq!(kg.node_count(), 10);
        assert_eq!(kg.get_node("e").unwrap().kind(), "entity");
        assert_eq!(kg.get_node("c").unwrap().kind(), "concept");
        assert_eq!(kg.get_node("f").unwrap().kind(), "fact");
        assert_eq!(kg.get_node("p").unwrap().kind(), "preference");
        assert_eq!(kg.get_node("pe").unwrap().kind(), "person");
        assert_eq!(kg.get_node("l").unwrap().kind(), "location");
        assert_eq!(kg.get_node("s").unwrap().kind(), "skill");
        assert_eq!(kg.get_node("t").unwrap().kind(), "tool");
        assert_eq!(kg.get_node("ev").unwrap().kind(), "event");
        assert_eq!(kg.get_node("n").unwrap().kind(), "note");
    }

    #[test]
    fn test_node_type_from_kind_and_key_normalizes_kind() {
        let node = NodeType::from_kind_and_key(" Person ", "alice").unwrap();
        assert_eq!(node.kind(), "person");
        assert_eq!(node.key(), "alice");
        assert!(NodeType::from_kind_and_key("unknown", "alice").is_none());
    }

    #[test]
    fn test_edge_types() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_node(NodeType::Entity("d".to_string()));

        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();
        kg.add_edge("b", "c", EdgeType::DependsOn).unwrap();
        kg.add_edge("c", "d", EdgeType::Prefers).unwrap();

        assert_eq!(kg.edge_count(), 3);
        let edges = kg.all_edges();
        let edge_kinds: Vec<&str> = edges
            .iter()
            .map(|(_, _, e)| match e {
                EdgeType::RelatesTo => "relates_to",
                EdgeType::DependsOn => "depends_on",
                EdgeType::Prefers => "prefers",
                _ => "other",
            })
            .collect();
        assert!(edge_kinds.contains(&"relates_to"));
        assert!(edge_kinds.contains(&"depends_on"));
        assert!(edge_kinds.contains(&"prefers"));
    }

    #[test]
    fn test_edge_type_relation_roundtrip_names() {
        let edge = EdgeType::from_relation(" Used_With ");
        assert_eq!(edge.relation_name(), "used_with");

        let custom = EdgeType::from_relation("supports");
        assert_eq!(custom.relation_name(), "supports");
    }

    #[test]
    fn test_stats() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_node(NodeType::Entity("c".to_string()));
        kg.add_edge("a", "b", EdgeType::RelatesTo).unwrap();

        let stats = kg.stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.connected_components, 2); // {a,b} and {c}
        // avg_degree = 2*edges/nodes = 2/3
        assert!((stats.avg_degree - 2.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_custom_edge() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        kg.add_node(NodeType::Entity("b".to_string()));
        kg.add_edge("a", "b", EdgeType::Custom("works_with".to_string()))
            .unwrap();

        assert_eq!(kg.edge_count(), 1);
        let edges = kg.all_edges();
        match edges[0].2 {
            EdgeType::Custom(s) => assert_eq!(s, "works_with"),
            _ => panic!("expected Custom edge"),
        }
    }

    #[test]
    fn test_find_path_same_node() {
        let mut kg = KnowledgeGraph::new();
        kg.add_node(NodeType::Entity("a".to_string()));
        let path = kg.find_path("a", "a").unwrap();
        assert_eq!(path, vec!["a"]);
    }

    #[test]
    fn test_query_neighbors_missing_node() {
        let kg = KnowledgeGraph::new();
        let neighbors = kg.query_neighbors("nonexistent", 3);
        assert!(neighbors.is_empty());
    }
}
