use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMetadata {
    /// Unique repository ID
    pub id: String,
    /// Name of the source file
    pub source_name: String,
    /// Original file size in bytes
    pub original_size: u64,
    /// Total number of lines in original
    pub original_line_count: usize,
    /// When the repository was created
    pub created_at: DateTime<Utc>,
    /// Optional description
    pub description: Option<String>,
}

impl RepoMetadata {
    pub fn new(source_name: String, original_size: u64, original_line_count: usize) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source_name,
            original_size,
            original_line_count,
            created_at: Utc::now(),
            description: None,
        }
    }
}
