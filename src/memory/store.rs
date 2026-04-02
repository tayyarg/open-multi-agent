//! In-memory implementation of the memory store.
//!
//! All data lives in a plain `HashMap` and is never persisted to disk. This is the
//! default store used by [`SharedMemory`] and is suitable for testing and
//! single-process use-cases.

use crate::types::MemoryEntry;
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Persistent (or in-memory) key-value store shared across agents.
/// Implementations may be backed by Redis, SQLite, or plain objects.
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn get(&self, key: &str) -> Option<MemoryEntry>;
    async fn set(&self, key: &str, value: &str, metadata: Option<HashMap<String, serde_json::Value>>);
    async fn list(&self) -> Vec<MemoryEntry>;
    async fn delete(&self, key: &str);
    async fn clear(&self);
}

/// Synchronous-under-the-hood key/value store that exposes an async surface
/// so implementations can be swapped for async-native backends without changing
/// callers.
#[derive(Clone)]
pub struct InMemoryStore {
    data: Arc<RwLock<HashMap<String, MemoryEntry>>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns entries whose key starts with `query` or whose value
    /// contains `query` (case-insensitive).
    pub async fn search(&self, query: &str) -> Vec<MemoryEntry> {
        let data = self.data.read().await;
        if query.is_empty() {
            return data.values().cloned().collect();
        }
        let lower = query.to_lowercase();
        data.values()
            .filter(|entry| {
                entry.key.to_lowercase().contains(&lower)
                    || entry.value.to_lowercase().contains(&lower)
            })
            .cloned()
            .collect()
    }

    /// Returns the number of entries currently held in the store.
    pub async fn size(&self) -> usize {
        self.data.read().await.len()
    }

    /// Returns `true` if `key` exists in the store.
    pub async fn has(&self, key: &str) -> bool {
        self.data.read().await.contains_key(key)
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryStore for InMemoryStore {
    async fn get(&self, key: &str) -> Option<MemoryEntry> {
        self.data.read().await.get(key).cloned()
    }

    async fn set(&self, key: &str, value: &str, metadata: Option<HashMap<String, serde_json::Value>>) {
        let mut data = self.data.write().await;
        let existing = data.get(key);
        let created_at = existing.map(|e| e.created_at).unwrap_or_else(Utc::now);
        let entry = MemoryEntry {
            key: key.to_string(),
            value: value.to_string(),
            metadata,
            created_at,
        };
        data.insert(key.to_string(), entry);
    }

    async fn list(&self) -> Vec<MemoryEntry> {
        self.data.read().await.values().cloned().collect()
    }

    async fn delete(&self, key: &str) {
        self.data.write().await.remove(key);
    }

    async fn clear(&self) {
        self.data.write().await.clear();
    }
}
