use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// A single text snippet stored in the local vector index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocument {
    pub id: String,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub embedding: Vec<f32>,
}

/// Search result returned from the local vector index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub score: f32,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Serializable JSON vector index.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorIndexFile {
    version: u32,
    embedding_model: String,
    embedding_dimension: usize,
    documents: Vec<VectorDocument>,
}

/// A lightweight local vector store backed by a JSON file.
///
/// This intentionally avoids introducing an external vector database. It is
/// suitable for agent memory / knowledge-graph snippets, and can be replaced by
/// Qdrant/LanceDB/pgvector later behind the same KnowledgeSearcher interface.
#[derive(Debug, Clone)]
pub struct VectorStore {
    path: PathBuf,
    embedding_model: String,
    embedding_dimension: usize,
    documents: Vec<VectorDocument>,
}

impl VectorStore {
    pub fn new(
        path: impl Into<PathBuf>,
        embedding_model: impl Into<String>,
        embedding_dimension: usize,
    ) -> Self {
        Self {
            path: path.into(),
            embedding_model: embedding_model.into(),
            embedding_dimension,
            documents: Vec::new(),
        }
    }

    pub fn sidecar_path(graph_path: &Path) -> PathBuf {
        let mut path = graph_path.to_path_buf();
        let filename = graph_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("knowledge.json");
        path.set_file_name(format!("{filename}.vectors.json"));
        path
    }

    pub fn load(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&self.path)?;
        let file: VectorIndexFile = serde_json::from_str(&data)?;
        self.embedding_model = file.embedding_model;
        self.embedding_dimension = file.embedding_dimension;
        self.documents = file.documents;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = VectorIndexFile {
            version: 1,
            embedding_model: self.embedding_model.clone(),
            embedding_dimension: self.embedding_dimension,
            documents: self.documents.clone(),
        };
        let json = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn embedding_model(&self) -> &str {
        &self.embedding_model
    }

    pub fn embedding_dimension(&self) -> usize {
        self.embedding_dimension
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    pub fn upsert(&mut self, document: VectorDocument) -> Result<()> {
        self.validate_embedding(&document.embedding)?;
        if let Some(existing) = self.documents.iter_mut().find(|d| d.id == document.id) {
            *existing = document;
        } else {
            self.documents.push(document);
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.documents.len();
        self.documents.retain(|d| d.id != id);
        before != self.documents.len()
    }

    pub fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<VectorSearchResult>> {
        self.validate_embedding(query_embedding)?;
        let mut results: Vec<VectorSearchResult> = self
            .documents
            .iter()
            .filter_map(|doc| {
                let score = cosine_similarity(query_embedding, &doc.embedding);
                score.is_finite().then(|| VectorSearchResult {
                    id: doc.id.clone(),
                    kind: doc.kind.clone(),
                    text: doc.text.clone(),
                    score,
                    metadata: doc.metadata.clone(),
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    fn validate_embedding(&self, embedding: &[f32]) -> Result<()> {
        if self.embedding_dimension > 0 && embedding.len() != self.embedding_dimension {
            anyhow::bail!(
                "embedding dimension mismatch: expected {}, got {}",
                self.embedding_dimension,
                embedding.len()
            );
        }
        Ok(())
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::NAN;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return f32::NAN;
    }
    (dot / (norm_a.sqrt() * norm_b.sqrt())) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn upsert_search_and_persist() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("knowledge.vectors.json");
        let mut store = VectorStore::new(&path, "test-embedding", 2);
        store
            .upsert(VectorDocument {
                id: "a".to_string(),
                kind: "entity".to_string(),
                text: "alpha".to_string(),
                metadata: json!({"key": "alpha"}),
                embedding: vec![1.0, 0.0],
            })
            .unwrap();
        store
            .upsert(VectorDocument {
                id: "b".to_string(),
                kind: "entity".to_string(),
                text: "beta".to_string(),
                metadata: json!({"key": "beta"}),
                embedding: vec![0.0, 1.0],
            })
            .unwrap();

        let results = store.search(&[0.9, 0.1], 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");

        store.save().unwrap();
        let mut reloaded = VectorStore::new(&path, "different", 999);
        reloaded.load().unwrap();
        assert_eq!(reloaded.embedding_model(), "test-embedding");
        assert_eq!(reloaded.embedding_dimension(), 2);
        assert_eq!(reloaded.document_count(), 2);
    }
}
