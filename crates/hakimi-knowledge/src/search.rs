use crate::graph::NodeType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Search result with relevance scoring and highlighting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub node_key: String,
    pub node_kind: String,
    pub score: f32,
    pub highlights: Vec<String>,
}

/// Options for search queries
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub limit: usize,
    pub min_score: Option<f32>,
    pub fuzzy: bool,
    pub case_sensitive: bool,
}

impl SearchOptions {
    pub fn new() -> Self {
        Self {
            limit: 20,
            min_score: None,
            fuzzy: false,
            case_sensitive: false,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_min_score(mut self, score: f32) -> Self {
        self.min_score = Some(score);
        self
    }

    pub fn with_fuzzy(mut self, fuzzy: bool) -> Self {
        self.fuzzy = fuzzy;
        self
    }

    pub fn with_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }
}

/// Search engine for knowledge graph nodes
pub struct SearchEngine;

impl SearchEngine {
    /// Search nodes with advanced scoring and ranking
    pub fn search(nodes: &[&NodeType], query: &str, options: &SearchOptions) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let query_terms = Self::tokenize(query);
        let mut results = Vec::new();

        for node in nodes {
            let key = node.key();
            let kind = node.kind();

            // Calculate relevance score
            let score = Self::calculate_score(key, &query_terms, options);

            if score > 0.0 {
                // Apply minimum score filter if set
                if let Some(min_score) = options.min_score {
                    if score < min_score {
                        continue;
                    }
                }

                // Generate highlights
                let highlights = Self::generate_highlights(key, &query_terms, options);

                results.push(SearchResult {
                    node_key: key.to_string(),
                    node_kind: kind.to_string(),
                    score,
                    highlights,
                });
            }
        }

        // Sort by score (descending)
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        results.truncate(options.limit);

        results
    }

    /// Tokenize text into searchable terms
    fn tokenize(text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    /// Calculate relevance score for a node key
    fn calculate_score(key: &str, query_terms: &[String], options: &SearchOptions) -> f32 {
        let text = if options.case_sensitive {
            key.to_string()
        } else {
            key.to_lowercase()
        };

        let normalized_terms: Vec<String> = query_terms
            .iter()
            .map(|t| {
                if options.case_sensitive {
                    t.clone()
                } else {
                    t.to_lowercase()
                }
            })
            .collect();

        let mut score = 0.0f32;

        for term in &normalized_terms {
            // Exact match (highest score)
            if text == *term {
                score += 10.0;
                continue;
            }

            // Full term contained (high score)
            if text.contains(term) {
                score += 5.0;

                // Bonus for position (earlier = better)
                if let Some(pos) = text.find(term) {
                    let position_bonus = 2.0 * (1.0 - (pos as f32 / text.len() as f32));
                    score += position_bonus;
                }

                // Bonus for term length (longer = more specific)
                let length_bonus = (term.len() as f32 / 10.0).min(2.0);
                score += length_bonus;

                continue;
            }

            // Fuzzy match (lower score)
            if options.fuzzy {
                let distance = Self::levenshtein_distance(&text, term);
                let max_len = text.len().max(term.len());
                let similarity = 1.0 - (distance as f32 / max_len as f32);

                // Only count fuzzy matches above 60% similarity
                if similarity > 0.6 {
                    score += similarity * 2.0;
                }
            }
        }

        score
    }

    /// Generate highlighted snippets of matching text
    fn generate_highlights(
        key: &str,
        query_terms: &[String],
        options: &SearchOptions,
    ) -> Vec<String> {
        let text = if options.case_sensitive {
            key.to_string()
        } else {
            key.to_lowercase()
        };

        let normalized_terms: Vec<String> = query_terms
            .iter()
            .map(|t| {
                if options.case_sensitive {
                    t.clone()
                } else {
                    t.to_lowercase()
                }
            })
            .collect();

        let mut highlights = Vec::new();
        let mut highlighted_text = key.to_string();

        // Find all matches and wrap them with markers
        for term in &normalized_terms {
            if text.contains(term) {
                // Simplified: just wrap matching term
                highlighted_text = highlighted_text.replace(
                    &key[..],
                    &key.replace(term, &format!("<mark>{}</mark>", term)),
                );
            }
        }

        // Extract snippet around matches
        if highlighted_text.contains("<mark>") {
            highlights.push(highlighted_text);
        }

        highlights
    }

    /// Calculate Levenshtein distance for fuzzy matching
    fn levenshtein_distance(s1: &str, s2: &str) -> usize {
        let len1 = s1.len();
        let len2 = s2.len();

        if len1 == 0 {
            return len2;
        }
        if len2 == 0 {
            return len1;
        }

        let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];

        for i in 0..=len1 {
            matrix[i][0] = i;
        }
        for j in 0..=len2 {
            matrix[0][j] = j;
        }

        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();

        for i in 1..=len1 {
            for j in 1..=len2 {
                let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                    0
                } else {
                    1
                };

                matrix[i][j] = (matrix[i - 1][j] + 1)
                    .min(matrix[i][j - 1] + 1)
                    .min(matrix[i - 1][j - 1] + cost);
            }
        }

        matrix[len1][len2]
    }
}

/// Search index for term frequency calculations
pub struct SearchIndex {
    // Term -> (node_key, frequency)
    term_frequencies: HashMap<String, HashMap<String, usize>>,
    // Total documents
    document_count: usize,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            term_frequencies: HashMap::new(),
            document_count: 0,
        }
    }

    /// Build index from nodes
    pub fn build(&mut self, nodes: &[&NodeType]) {
        self.term_frequencies.clear();
        self.document_count = nodes.len();

        for node in nodes {
            let key = node.key();
            let terms = SearchEngine::tokenize(key);

            for term in terms {
                let term_lower = term.to_lowercase();
                let doc_freqs = self
                    .term_frequencies
                    .entry(term_lower)
                    .or_insert_with(HashMap::new);
                *doc_freqs.entry(key.to_string()).or_insert(0) += 1;
            }
        }
    }

    /// Get TF-IDF score for a term in a document
    pub fn tf_idf(&self, term: &str, doc_key: &str) -> f32 {
        let term_lower = term.to_lowercase();

        // Term frequency
        let tf = if let Some(doc_freqs) = self.term_frequencies.get(&term_lower) {
            *doc_freqs.get(doc_key).unwrap_or(&0) as f32
        } else {
            0.0
        };

        if tf == 0.0 {
            return 0.0;
        }

        // Document frequency (in how many docs does the term appear)
        let df = if let Some(doc_freqs) = self.term_frequencies.get(&term_lower) {
            doc_freqs.len() as f32
        } else {
            1.0
        };

        // IDF (inverse document frequency)
        let idf = ((self.document_count as f32 + 1.0) / (df + 1.0)).ln();

        tf * idf
    }

    /// Search using TF-IDF scoring
    pub fn search_tfidf(
        &self,
        nodes: &[&NodeType],
        query: &str,
        options: &SearchOptions,
    ) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let query_terms = SearchEngine::tokenize(query);
        let mut results = Vec::new();

        for node in nodes {
            let key = node.key();
            let kind = node.kind();

            // Calculate TF-IDF score
            let mut score = 0.0f32;
            for term in &query_terms {
                score += self.tf_idf(term, key);
            }

            if score > 0.0 {
                if let Some(min_score) = options.min_score {
                    if score < min_score {
                        continue;
                    }
                }

                let highlights = SearchEngine::generate_highlights(key, &query_terms, options);

                results.push(SearchResult {
                    node_key: key.to_string(),
                    node_kind: kind.to_string(),
                    score,
                    highlights,
                });
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(options.limit);

        results
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let terms = SearchEngine::tokenize("Hello, world! This is a test.");
        assert_eq!(terms, vec!["Hello", "world", "This", "is", "a", "test"]);
    }

    #[test]
    fn test_simple_search() {
        let node1 = NodeType::Entity("alice".to_string());
        let node2 = NodeType::Entity("bob".to_string());
        let node3 = NodeType::Entity("alice_in_wonderland".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new().with_limit(10);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].node_key, "alice"); // Exact match scores higher
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_fuzzy_search() {
        let node1 = NodeType::Entity("alice".to_string());
        let node2 = NodeType::Entity("alica".to_string()); // Typo
        let node3 = NodeType::Entity("bob".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new().with_limit(10).with_fuzzy(true);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert!(results.len() >= 2);
        assert_eq!(results[0].node_key, "alice");
        // "alica" should also match with fuzzy search
    }

    #[test]
    fn test_case_insensitive_search() {
        let node1 = NodeType::Entity("Alice".to_string());
        let node2 = NodeType::Entity("ALICE".to_string());
        let node3 = NodeType::Entity("alice".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new().with_limit(10);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_case_sensitive_search() {
        let node1 = NodeType::Entity("Alice".to_string());
        let node2 = NodeType::Entity("alice".to_string());
        let node3 = NodeType::Entity("ALICE".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new()
            .with_limit(10)
            .with_case_sensitive(true);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_key, "alice");
    }

    #[test]
    fn test_min_score_filter() {
        let node1 = NodeType::Entity("alice".to_string());
        let node2 = NodeType::Entity("alice_wonderland".to_string());
        let node3 = NodeType::Entity("bob".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new().with_limit(10).with_min_score(5.0);
        let results = SearchEngine::search(&nodes, "alice", &options);

        // Only high-scoring results should pass
        assert!(!results.is_empty());
        for result in &results {
            assert!(result.score >= 5.0);
        }
    }

    #[test]
    fn test_limit() {
        let node1 = NodeType::Entity("alice1".to_string());
        let node2 = NodeType::Entity("alice2".to_string());
        let node3 = NodeType::Entity("alice3".to_string());
        let node4 = NodeType::Entity("alice4".to_string());
        let nodes = vec![&node1, &node2, &node3, &node4];

        let options = SearchOptions::new().with_limit(2);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(SearchEngine::levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(SearchEngine::levenshtein_distance("saturday", "sunday"), 3);
        assert_eq!(SearchEngine::levenshtein_distance("", "abc"), 3);
        assert_eq!(SearchEngine::levenshtein_distance("abc", "abc"), 0);
    }

    #[test]
    fn test_search_index_build() {
        let node1 = NodeType::Entity("alice wonderland".to_string());
        let node2 = NodeType::Entity("bob builder".to_string());
        let node3 = NodeType::Entity("alice cooper".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let mut index = SearchIndex::new();
        index.build(&nodes);

        assert_eq!(index.document_count, 3);
        // "alice" appears in 2 documents
        assert!(index.term_frequencies.contains_key("alice"));
    }

    #[test]
    fn test_tfidf_search() {
        let node1 = NodeType::Entity("alice wonderland".to_string());
        let node2 = NodeType::Entity("bob builder".to_string());
        let node3 = NodeType::Entity("alice cooper".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let mut index = SearchIndex::new();
        index.build(&nodes);

        let options = SearchOptions::new().with_limit(10);
        let results = index.search_tfidf(&nodes, "alice", &options);

        assert_eq!(results.len(), 2);
        // Both "alice wonderland" and "alice cooper" should match
    }

    #[test]
    fn test_highlights() {
        let node1 = NodeType::Entity("alice_in_wonderland".to_string());
        let nodes = vec![&node1];

        let options = SearchOptions::new().with_limit(10);
        let results = SearchEngine::search(&nodes, "alice", &options);

        assert_eq!(results.len(), 1);
        assert!(!results[0].highlights.is_empty());
        // Highlight should contain <mark> tags
        assert!(results[0].highlights[0].contains("<mark>"));
    }

    #[test]
    fn test_multi_term_search() {
        let node1 = NodeType::Entity("alice in wonderland".to_string());
        let node2 = NodeType::Entity("alice cooper".to_string());
        let node3 = NodeType::Entity("wonderland resort".to_string());
        let nodes = vec![&node1, &node2, &node3];

        let options = SearchOptions::new().with_limit(10);
        let results = SearchEngine::search(&nodes, "alice wonderland", &options);

        // "alice in wonderland" should score highest (contains both terms)
        assert!(!results.is_empty());
        assert_eq!(results[0].node_key, "alice in wonderland");
    }

    #[test]
    fn test_empty_query() {
        let node1 = NodeType::Entity("alice".to_string());
        let nodes = vec![&node1];

        let options = SearchOptions::new();
        let results = SearchEngine::search(&nodes, "", &options);

        assert!(results.is_empty());
    }

    #[test]
    fn test_no_matches() {
        let node1 = NodeType::Entity("alice".to_string());
        let nodes = vec![&node1];

        let options = SearchOptions::new();
        let results = SearchEngine::search(&nodes, "xyz", &options);

        assert!(results.is_empty());
    }
}
