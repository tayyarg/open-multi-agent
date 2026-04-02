//! Agent pool for managing and scheduling multiple agents.

use crate::agent::agent::Agent;
use crate::types::AgentRunResult;
use crate::utils::semaphore::Semaphore;
use std::collections::HashMap;
use std::sync::Arc;

/// Snapshot of pool health.
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub total: usize,
    pub idle: usize,
    pub running: usize,
    pub completed: usize,
    pub error: usize,
}

/// Registry and scheduler for a collection of [`Agent`] instances.
pub struct AgentPool {
    agents: HashMap<String, Arc<Agent>>,
    semaphore: Semaphore,
    #[allow(dead_code)]
    round_robin_index: usize,
}

impl AgentPool {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            agents: HashMap::new(),
            semaphore: Semaphore::new(max_concurrency.max(1)),
            round_robin_index: 0,
        }
    }

    /// Register an agent with the pool.
    pub fn add(&mut self, agent: Arc<Agent>) {
        if self.agents.contains_key(&agent.name) {
            panic!(
                "AgentPool: agent '{}' is already registered.",
                agent.name
            );
        }
        self.agents.insert(agent.name.clone(), agent);
    }

    /// Unregister an agent by name.
    pub fn remove(&mut self, name: &str) {
        self.agents.remove(name);
    }

    /// Retrieve a registered agent by name.
    pub fn get(&self, name: &str) -> Option<Arc<Agent>> {
        self.agents.get(name).cloned()
    }

    /// Return all registered agents.
    pub fn list(&self) -> Vec<Arc<Agent>> {
        self.agents.values().cloned().collect()
    }

    /// Run a single prompt on the named agent.
    pub async fn run(&self, agent_name: &str, prompt: &str) -> Result<AgentRunResult, String> {
        let agent = self
            .agents
            .get(agent_name)
            .ok_or_else(|| {
                format!(
                    "AgentPool: agent '{}' is not registered. Registered: [{}]",
                    agent_name,
                    self.agents.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?
            .clone();

        self.semaphore.acquire().await;
        let result = agent.run(prompt).await;
        self.semaphore.release().await;
        Ok(result)
    }

    /// Run prompts on multiple agents in parallel.
    pub async fn run_parallel(
        &self,
        tasks: Vec<(String, String)>,
    ) -> HashMap<String, AgentRunResult> {
        let mut handles = Vec::new();

        for (agent_name, prompt) in tasks {
            let agent = match self.agents.get(&agent_name) {
                Some(a) => a.clone(),
                None => continue,
            };
            let sem = self.semaphore.clone();
            let name = agent_name.clone();

            let handle = tokio::spawn(async move {
                sem.acquire().await;
                let result = agent.run(&prompt).await;
                sem.release().await;
                (name, result)
            });
            handles.push(handle);
        }

        let mut results = HashMap::new();
        for handle in handles {
            if let Ok((name, result)) = handle.await {
                results.insert(name, result);
            }
        }
        results
    }

    /// Reset all agents in the pool.
    pub async fn shutdown(&self) {
        for agent in self.agents.values() {
            agent.reset().await;
        }
    }

    /// Snapshot of agent statuses.
    pub async fn get_status(&self) -> PoolStatus {
        use crate::types::AgentStatus;
        let mut idle = 0;
        let mut running = 0;
        let mut completed = 0;
        let mut error = 0;

        for agent in self.agents.values() {
            match agent.get_state().await.status {
                AgentStatus::Idle => idle += 1,
                AgentStatus::Running => running += 1,
                AgentStatus::Completed => completed += 1,
                AgentStatus::Error => error += 1,
            }
        }

        PoolStatus {
            total: self.agents.len(),
            idle,
            running,
            completed,
            error,
        }
    }
}
