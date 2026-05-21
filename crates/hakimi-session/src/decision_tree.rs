use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub message: String,
    pub role: String,
    pub tool_calls: Vec<String>,
    pub outcome: Outcome,
    pub metadata: HashMap<String, String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Outcome {
    Pending,
    Success,
    Failure(String),
    Corrected,
    Abandoned,
}

pub struct DecisionTree {
    nodes: HashMap<String, DecisionNode>,
    root_id: String,
    current_id: String,
    children: HashMap<String, Vec<String>>,
    id_counter: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathComparison {
    pub common_ancestor: String,
    pub path_a_summary: Vec<String>,
    pub path_b_summary: Vec<String>,
    pub divergence_point: String,
}

/// Serializable snapshot for JSON round-tripping.
#[derive(Serialize, Deserialize)]
struct DecisionTreeSnapshot {
    nodes: HashMap<String, DecisionNode>,
    root_id: String,
    current_id: String,
    children: HashMap<String, Vec<String>>,
    id_counter: u64,
}

impl DecisionTree {
    /// Create a new decision tree with an initial root node.
    pub fn new(initial_message: &str) -> Self {
        let mut nodes = HashMap::new();
        let mut children = HashMap::new();

        let root = DecisionNode {
            id: "n0".to_string(),
            parent_id: None,
            message: initial_message.to_string(),
            role: "user".to_string(),
            tool_calls: Vec::new(),
            outcome: Outcome::Pending,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let root_id = root.id.clone();
        children.insert(root_id.clone(), Vec::new());
        nodes.insert(root_id.clone(), root);

        Self {
            nodes,
            root_id: root_id.clone(),
            current_id: root_id,
            children,
            id_counter: 1,
        }
    }

    /// Generate the next sequential node ID.
    pub fn next_id(&mut self) -> String {
        let id = format!("n{}", self.id_counter);
        self.id_counter += 1;
        id
    }

    /// Add a new node as a child of the current node and advance current.
    pub fn add_node(
        &mut self,
        message: &str,
        role: &str,
        tool_calls: Vec<String>,
        outcome: Outcome,
    ) -> String {
        let id = self.next_id();
        let parent_id = self.current_id.clone();

        let node = DecisionNode {
            id: id.clone(),
            parent_id: Some(parent_id.clone()),
            message: message.to_string(),
            role: role.to_string(),
            tool_calls,
            outcome,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.children.insert(id.clone(), Vec::new());
        self.children.entry(parent_id).or_default().push(id.clone());
        self.nodes.insert(id.clone(), node);
        self.current_id = id.clone();
        id
    }

    /// Set current to the node with the given ID.
    pub fn backtrack_to(&mut self, node_id: &str) -> anyhow::Result<()> {
        if !self.nodes.contains_key(node_id) {
            anyhow::bail!("node not found: {}", node_id);
        }
        self.current_id = node_id.to_string();
        Ok(())
    }

    /// Get a reference to the current node.
    pub fn get_current(&self) -> &DecisionNode {
        self.nodes
            .get(&self.current_id)
            .expect("current node must exist")
    }

    /// Get a reference to a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&DecisionNode> {
        self.nodes.get(id)
    }

    /// Get the path from root to the given node.
    pub fn get_path(&self, node_id: &str) -> Vec<&DecisionNode> {
        let mut path = Vec::new();
        let mut current = node_id;

        // Collect ancestors in reverse
        while let Some(node) = self.nodes.get(current) {
            path.push(node);
            match &node.parent_id {
                Some(pid) => current = pid,
                None => break,
            }
        }

        path.reverse();
        path
    }

    /// Get children of a node.
    pub fn get_children(&self, node_id: &str) -> Vec<&DecisionNode> {
        self.children
            .get(node_id)
            .map(|kids| kids.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Compare two paths and find their divergence point.
    pub fn compare_paths(&self, node_a: &str, node_b: &str) -> PathComparison {
        let path_a = self.get_path(node_a);
        let path_b = self.get_path(node_b);

        // Find last common ancestor
        let mut common_idx = 0;
        let min_len = path_a.len().min(path_b.len());
        for i in 0..min_len {
            if path_a[i].id == path_b[i].id {
                common_idx = i;
            } else {
                break;
            }
        }

        let common_ancestor = path_a[common_idx].id.clone();
        let divergence_point = if common_idx + 1 < path_a.len() {
            path_a[common_idx + 1].id.clone()
        } else if common_idx + 1 < path_b.len() {
            path_b[common_idx + 1].id.clone()
        } else {
            common_ancestor.clone()
        };

        let path_a_summary: Vec<String> = path_a[common_idx..]
            .iter()
            .map(|n| n.message.clone())
            .collect();
        let path_b_summary: Vec<String> = path_b[common_idx..]
            .iter()
            .map(|n| n.message.clone())
            .collect();

        PathComparison {
            common_ancestor,
            path_a_summary,
            path_b_summary,
            divergence_point,
        }
    }

    /// Update the outcome of a node.
    pub fn mark_outcome(&mut self, node_id: &str, outcome: Outcome) -> anyhow::Result<()> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| anyhow::anyhow!("node not found: {}", node_id))?;
        node.outcome = outcome;
        Ok(())
    }

    /// Get siblings of a node (other children of the same parent).
    pub fn get_siblings(&self, node_id: &str) -> Vec<&DecisionNode> {
        let node = match self.nodes.get(node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let parent_id = match &node.parent_id {
            Some(pid) => pid,
            None => return Vec::new(), // root has no siblings
        };

        self.children
            .get(parent_id)
            .map(|kids| {
                kids.iter()
                    .filter(|id| *id != node_id)
                    .filter_map(|id| self.nodes.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Serialize the tree to JSON.
    pub fn to_json(&self) -> anyhow::Result<String> {
        let snapshot = DecisionTreeSnapshot {
            nodes: self.nodes.clone(),
            root_id: self.root_id.clone(),
            current_id: self.current_id.clone(),
            children: self.children.clone(),
            id_counter: self.id_counter,
        };
        Ok(serde_json::to_string(&snapshot)?)
    }

    /// Deserialize a tree from JSON.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let snapshot: DecisionTreeSnapshot = serde_json::from_str(json)?;
        Ok(Self {
            nodes: snapshot.nodes,
            root_id: snapshot.root_id,
            current_id: snapshot.current_id,
            children: snapshot.children,
            id_counter: snapshot.id_counter,
        })
    }

    /// Depth from root to current node.
    pub fn depth(&self) -> usize {
        self.get_path(&self.current_id).len().saturating_sub(1)
    }

    /// Total number of nodes in the tree.
    pub fn total_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Number of nodes that have 2 or more children (branch points).
    pub fn branches(&self) -> usize {
        self.children
            .values()
            .filter(|kids| kids.len() >= 2)
            .count()
    }

    /// Get message summaries from root to current.
    pub fn current_branch_summary(&self) -> Vec<String> {
        self.get_path(&self.current_id)
            .iter()
            .map(|n| n.message.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tree() {
        let tree = DecisionTree::new("Hello");
        assert_eq!(tree.root_id, "n0");
        assert_eq!(tree.current_id, "n0");
        assert_eq!(tree.total_nodes(), 1);
        assert_eq!(tree.get_current().message, "Hello");
        assert_eq!(tree.get_current().role, "user");
    }

    #[test]
    fn test_add_nodes_linear() {
        let mut tree = DecisionTree::new("start");
        let id1 = tree.add_node("step 1", "assistant", vec![], Outcome::Pending);
        assert_eq!(id1, "n1");
        let id2 = tree.add_node("step 2", "user", vec![], Outcome::Pending);
        assert_eq!(id2, "n2");
        assert_eq!(tree.total_nodes(), 3);
        assert_eq!(tree.get_current().message, "step 2");
    }

    #[test]
    fn test_add_branching() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("child a", "user", vec![], Outcome::Pending);
        let child_a_id = tree.get_current().id.clone();

        tree.backtrack_to("n0").unwrap();
        tree.add_node("child b", "user", vec![], Outcome::Pending);
        let child_b_id = tree.get_current().id.clone();

        assert_eq!(tree.get_children("n0").len(), 2);
        assert_ne!(child_a_id, child_b_id);
    }

    #[test]
    fn test_backtrack() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("child", "assistant", vec![], Outcome::Success);
        assert_eq!(tree.get_current().id, "n1");

        tree.backtrack_to("n0").unwrap();
        assert_eq!(tree.get_current().id, "n0");
    }

    #[test]
    fn test_backtrack_invalid() {
        let mut tree = DecisionTree::new("root");
        let result = tree.backtrack_to("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_path() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("a", "user", vec![], Outcome::Pending);
        tree.add_node("b", "assistant", vec![], Outcome::Pending);
        tree.add_node("c", "user", vec![], Outcome::Pending);

        let path = tree.get_path("n3");
        assert_eq!(path.len(), 4);
        assert_eq!(path[0].id, "n0");
        assert_eq!(path[1].id, "n1");
        assert_eq!(path[2].id, "n2");
        assert_eq!(path[3].id, "n3");
    }

    #[test]
    fn test_compare_paths() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("a", "user", vec![], Outcome::Pending);
        let a_id = tree.get_current().id.clone();

        tree.backtrack_to("n0").unwrap();
        tree.add_node("b", "user", vec![], Outcome::Pending);
        let b_id = tree.get_current().id.clone();

        let comparison = tree.compare_paths(&a_id, &b_id);
        assert_eq!(comparison.common_ancestor, "n0");
        assert!(comparison.path_a_summary.contains(&"a".to_string()));
        assert!(comparison.path_b_summary.contains(&"b".to_string()));
    }

    #[test]
    fn test_get_children() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("child1", "user", vec![], Outcome::Pending);
        tree.backtrack_to("n0").unwrap();
        tree.add_node("child2", "user", vec![], Outcome::Pending);

        let children = tree.get_children("n0");
        assert_eq!(children.len(), 2);

        let leaf_children = tree.get_children("n1");
        assert_eq!(leaf_children.len(), 0);
    }

    #[test]
    fn test_get_siblings() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("child1", "user", vec![], Outcome::Pending);
        let child1_id = tree.get_current().id.clone();

        tree.backtrack_to("n0").unwrap();
        tree.add_node("child2", "user", vec![], Outcome::Pending);
        let child2_id = tree.get_current().id.clone();

        let siblings_of_child1 = tree.get_siblings(&child1_id);
        assert_eq!(siblings_of_child1.len(), 1);
        assert_eq!(siblings_of_child1[0].id, child2_id);

        // Root has no siblings
        let root_siblings = tree.get_siblings("n0");
        assert_eq!(root_siblings.len(), 0);
    }

    #[test]
    fn test_mark_outcome() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("step", "assistant", vec![], Outcome::Pending);

        assert_eq!(tree.get_node("n1").unwrap().outcome, Outcome::Pending);

        tree.mark_outcome("n1", Outcome::Success).unwrap();
        assert_eq!(tree.get_node("n1").unwrap().outcome, Outcome::Success);

        tree.mark_outcome("n1", Outcome::Failure("oops".into()))
            .unwrap();
        assert_eq!(
            tree.get_node("n1").unwrap().outcome,
            Outcome::Failure("oops".into())
        );

        assert!(tree.mark_outcome("nonexistent", Outcome::Success).is_err());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut tree = DecisionTree::new("root");
        tree.add_node("step1", "user", vec!["tool_a".into()], Outcome::Pending);
        tree.add_node("step2", "assistant", vec![], Outcome::Success);

        let json = tree.to_json().unwrap();
        let mut restored = DecisionTree::from_json(&json).unwrap();

        assert_eq!(restored.total_nodes(), 3);
        assert_eq!(restored.get_current().message, "step2");
        assert_eq!(restored.get_current().outcome, Outcome::Success);
        assert_eq!(restored.get_node("n1").unwrap().tool_calls, vec!["tool_a"]);

        // Can continue building the restored tree
        restored.add_node("step3", "user", vec![], Outcome::Pending);
        assert_eq!(restored.total_nodes(), 4);
    }

    #[test]
    fn test_depth() {
        let mut tree = DecisionTree::new("root");
        assert_eq!(tree.depth(), 0);

        tree.add_node("a", "user", vec![], Outcome::Pending);
        assert_eq!(tree.depth(), 1);

        tree.add_node("b", "assistant", vec![], Outcome::Pending);
        assert_eq!(tree.depth(), 2);

        tree.backtrack_to("n0").unwrap();
        assert_eq!(tree.depth(), 0);
    }

    #[test]
    fn test_total_nodes() {
        let mut tree = DecisionTree::new("root");
        assert_eq!(tree.total_nodes(), 1);

        tree.add_node("a", "user", vec![], Outcome::Pending);
        assert_eq!(tree.total_nodes(), 2);

        tree.backtrack_to("n0").unwrap();
        tree.add_node("b", "user", vec![], Outcome::Pending);
        assert_eq!(tree.total_nodes(), 3);
    }

    #[test]
    fn test_branches() {
        let mut tree = DecisionTree::new("root");
        assert_eq!(tree.branches(), 0);

        tree.add_node("a", "user", vec![], Outcome::Pending);
        assert_eq!(tree.branches(), 0);

        tree.backtrack_to("n0").unwrap();
        tree.add_node("b", "user", vec![], Outcome::Pending);
        assert_eq!(tree.branches(), 1); // n0 now has 2 children
    }

    #[test]
    fn test_current_branch_summary() {
        let mut tree = DecisionTree::new("start");
        tree.add_node("middle", "user", vec![], Outcome::Pending);
        tree.add_node("end", "assistant", vec![], Outcome::Pending);

        let summary = tree.current_branch_summary();
        assert_eq!(summary, vec!["start", "middle", "end"]);
    }

    #[test]
    fn test_complex_tree_3_levels() {
        let mut tree = DecisionTree::new("task");

        // Level 1 children
        tree.add_node("approach A", "user", vec![], Outcome::Pending);
        let a_id = tree.get_current().id.clone();

        // Level 2 under A
        tree.add_node(
            "step A1",
            "assistant",
            vec!["read".into()],
            Outcome::Success,
        );
        let a1_id = tree.get_current().id.clone();

        tree.backtrack_to(&a_id).unwrap();
        tree.add_node(
            "step A2",
            "assistant",
            vec!["write".into()],
            Outcome::Pending,
        );
        let a2_id = tree.get_current().id.clone();

        // Back to root, create level 1 child B
        tree.backtrack_to("n0").unwrap();
        tree.add_node("approach B", "user", vec![], Outcome::Pending);
        let b_id = tree.get_current().id.clone();

        // Level 2 under B
        tree.add_node(
            "step B1",
            "assistant",
            vec!["search".into()],
            Outcome::Pending,
        );

        // Verify structure
        assert_eq!(tree.total_nodes(), 6);
        assert_eq!(tree.branches(), 2); // root + approach A

        let children_root = tree.get_children("n0");
        assert_eq!(children_root.len(), 2);

        let children_a = tree.get_children(&a_id);
        assert_eq!(children_a.len(), 2);

        let siblings_a1 = tree.get_siblings(&a1_id);
        assert_eq!(siblings_a1.len(), 1);
        assert_eq!(siblings_a1[0].id, a2_id);

        let comparison = tree.compare_paths(&a1_id, &b_id);
        assert_eq!(comparison.common_ancestor, "n0");

        // Verify path depths
        assert_eq!(tree.get_path(&a1_id).len(), 3);
        assert_eq!(tree.get_path("n0").len(), 1);

        // Verify outcomes
        assert_eq!(tree.get_node(&a1_id).unwrap().outcome, Outcome::Success);
    }
}
