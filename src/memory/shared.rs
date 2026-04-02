//! Shared memory layer for teams of cooperating agents.
//!
//! Each agent writes under its own namespace (`<agent_name>/<key>`) so entries
//! remain attributable, while any agent may read any entry.

use crate::memory::store::{InMemoryStore, MemoryStore};
use crate::types::MemoryEntry;
use std::collections::HashMap;

/// Namespaced shared memory for a team of agents.
///
/// Writes are namespaced as `<agent_name>/<key>` so that entries from different
/// agents never collide and are always attributable. Reads are namespace-aware
/// but also accept fully-qualified keys.
pub struct SharedMemory {
    store: InMemoryStore,
}

impl SharedMemory {
    pub fn new() -> Self {
        Self {
            store: InMemoryStore::new(),
        }
    }

    /// Write `value` under the namespaced key `<agent_name>/<key>`.
    pub async fn write(
        &self,
        agent_name: &str,
        key: &str,
        value: &str,
        metadata: Option<HashMap<String, serde_json::Value>>,
    ) {
        let namespaced_key = Self::namespace_key(agent_name, key);
        let mut meta = metadata.unwrap_or_default();
        meta.insert(
            "agent".to_string(),
            serde_json::Value::String(agent_name.to_string()),
        );
        self.store.set(&namespaced_key, value, Some(meta)).await;
    }

    /// Read an entry by its fully-qualified key (`<agent_name>/<key>`).
    pub async fn read(&self, key: &str) -> Option<MemoryEntry> {
        self.store.get(key).await
    }

    /// Returns every entry in the shared store, regardless of agent.
    pub async fn list_all(&self) -> Vec<MemoryEntry> {
        self.store.list().await
    }

    /// Returns all entries written by `agent_name`.
    pub async fn list_by_agent(&self, agent_name: &str) -> Vec<MemoryEntry> {
        let prefix = Self::namespace_key(agent_name, "");
        let all = self.store.list().await;
        all.into_iter()
            .filter(|entry| entry.key.starts_with(&prefix))
            .collect()
    }

    /// Produces a human-readable summary of all entries in the store.
    pub async fn get_summary(&self) -> String {
        let all = self.store.list().await;
        if all.is_empty() {
            return String::new();
        }

        let mut by_agent: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for entry in &all {
            let (agent, local_key) = match entry.key.find('/') {
                Some(idx) => (
                    entry.key[..idx].to_string(),
                    entry.key[idx + 1..].to_string(),
                ),
                None => ("_unknown".to_string(), entry.key.clone()),
            };

            by_agent
                .entry(agent)
                .or_default()
                .push((local_key, entry.value.clone()));
        }

        let mut lines = vec!["## Shared Team Memory".to_string(), String::new()];
        for (agent, entries) in &by_agent {
            lines.push(format!("### {}", agent));
            for (local_key, value) in entries {
                let display_value = if value.len() > 200 {
                    format!("{}…", &value[..197])
                } else {
                    value.clone()
                };
                lines.push(format!("- {}: {}", local_key, display_value));
            }
            lines.push(String::new());
        }

        lines.join("\n").trim_end().to_string()
    }

    /// Returns the underlying [`InMemoryStore`].
    pub fn get_store(&self) -> &InMemoryStore {
        &self.store
    }

    fn namespace_key(agent_name: &str, key: &str) -> String {
        format!("{}/{}", agent_name, key)
    }
}

impl Default for SharedMemory {
    fn default() -> Self {
        Self::new()
    }
}
