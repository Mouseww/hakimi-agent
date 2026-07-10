use serde::{Deserialize, Serialize};

/// Represents a version of a knowledge entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeVersion {
    pub id: String,
    pub knowledge_key: String,
    pub version: i32,
    pub content: String,
    pub metadata: serde_json::Value,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub change_summary: Option<String>,
}

impl KnowledgeVersion {
    pub fn new(
        knowledge_key: String,
        version: i32,
        content: String,
        metadata: serde_json::Value,
        change_summary: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            knowledge_key,
            version,
            content,
            metadata,
            created_at: chrono::Utc::now().timestamp(),
            created_by: None,
            change_summary,
        }
    }
}

/// Metadata attached to a knowledge node for versioning
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VersionedMetadata {
    pub current_version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

impl VersionedMetadata {
    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            current_version: 1,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn increment_version(&mut self) {
        self.current_version += 1;
        self.updated_at = chrono::Utc::now().timestamp();
    }
}
