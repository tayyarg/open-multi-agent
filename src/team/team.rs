//! Team — the central coordination object for a named group of agents.

use crate::memory::shared::SharedMemory;
use crate::task::queue::TaskQueue;
use crate::task::task::create_task;
use crate::team::messaging::{Message, MessageBus};
use crate::types::{AgentConfig, Task, TaskStatus, TeamConfig};
use std::collections::HashMap;

/// Coordinates a named group of agents with shared messaging, task queuing,
/// and optional shared memory.
pub struct Team {
    pub name: String,
    pub config: TeamConfig,
    agent_map: HashMap<String, AgentConfig>,
    bus: MessageBus,
    queue: TaskQueue,
    memory: Option<SharedMemory>,
}

impl Team {
    pub fn new(config: TeamConfig) -> Self {
        let agent_map: HashMap<String, AgentConfig> = config
            .agents
            .iter()
            .map(|a| (a.name.clone(), a.clone()))
            .collect();

        let memory = if config.shared_memory {
            Some(SharedMemory::new())
        } else {
            None
        };

        Self {
            name: config.name.clone(),
            config,
            agent_map,
            bus: MessageBus::new(),
            queue: TaskQueue::new(),
            memory,
        }
    }

    // --- Agent roster ---

    /// Returns a copy of the agent configs.
    pub fn get_agents(&self) -> Vec<AgentConfig> {
        self.agent_map.values().cloned().collect()
    }

    /// Looks up an agent by name.
    pub fn get_agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agent_map.get(name)
    }

    // --- Messaging ---

    /// Sends a point-to-point message.
    pub fn send_message(&mut self, from: &str, to: &str, content: &str) {
        self.bus.send(from, to, content);
    }

    /// Returns all messages addressed to `agent_name`.
    pub fn get_messages(&self, agent_name: &str) -> Vec<Message> {
        self.bus.get_all(agent_name).into_iter().cloned().collect()
    }

    /// Broadcasts a message to all agents.
    pub fn broadcast(&mut self, from: &str, content: &str) {
        self.bus.broadcast(from, content);
    }

    // --- Task management ---

    /// Creates a new task and adds it to the queue.
    pub fn add_task(
        &mut self,
        title: &str,
        description: &str,
        status: TaskStatus,
        assignee: Option<&str>,
        depends_on: Option<Vec<String>>,
    ) -> Task {
        let mut task = create_task(title, description, assignee, depends_on);
        if status != TaskStatus::Pending {
            task.status = status;
        }
        self.queue.add(task.clone());
        task
    }

    /// Returns all tasks in the queue.
    pub fn get_tasks(&self) -> Vec<Task> {
        self.queue.list()
    }

    /// Returns tasks assigned to an agent.
    pub fn get_tasks_by_assignee(&self, agent_name: &str) -> Vec<Task> {
        self.queue
            .list()
            .into_iter()
            .filter(|t| t.assignee.as_deref() == Some(agent_name))
            .collect()
    }

    /// Returns the next pending task for an agent.
    pub fn get_next_task(&self, agent_name: &str) -> Option<Task> {
        self.queue
            .next(Some(agent_name))
            .or_else(|| self.queue.next_available())
            .cloned()
    }

    // --- Memory ---

    /// Returns the shared memory instance, if enabled.
    pub fn get_shared_memory_instance(&self) -> Option<&SharedMemory> {
        self.memory.as_ref()
    }

    /// Returns a mutable reference to the task queue.
    pub fn get_queue_mut(&mut self) -> &mut TaskQueue {
        &mut self.queue
    }

    /// Returns an immutable reference to the task queue.
    pub fn get_queue(&self) -> &TaskQueue {
        &self.queue
    }
}
