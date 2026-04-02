//! Inter-agent message bus.
//!
//! Provides a lightweight pub/sub system so agents can exchange typed messages
//! without direct references to each other.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A single message exchanged between agents (or broadcast to all).
#[derive(Debug, Clone)]
pub struct Message {
    /// Stable UUID for this message.
    pub id: String,
    /// Name of the sending agent.
    pub from: String,
    /// Recipient agent name, or `"*"` for broadcast.
    pub to: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// In-memory message bus for inter-agent communication.
pub struct MessageBus {
    messages: Vec<Message>,
    read_state: std::collections::HashMap<String, std::collections::HashSet<String>>,
    subscribers: std::collections::HashMap<String, Vec<Box<dyn Fn(&Message) + Send + Sync>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            read_state: std::collections::HashMap::new(),
            subscribers: std::collections::HashMap::new(),
        }
    }

    /// Send a message from `from` to `to`.
    pub fn send(&mut self, from: &str, to: &str, content: &str) -> Message {
        let message = Message {
            id: Uuid::new_v4().to_string(),
            from: from.to_string(),
            to: to.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
        };
        self.persist(message.clone());
        message
    }

    /// Broadcast a message from `from` to all other agents.
    pub fn broadcast(&mut self, from: &str, content: &str) -> Message {
        self.send(from, "*", content)
    }

    /// Returns unread messages for `agent_name`.
    pub fn get_unread(&self, agent_name: &str) -> Vec<&Message> {
        let read = self.read_state.get(agent_name);
        self.messages
            .iter()
            .filter(|m| {
                is_addressed_to(m, agent_name)
                    && read.map_or(true, |r| !r.contains(&m.id))
            })
            .collect()
    }

    /// Returns every message addressed to `agent_name`.
    pub fn get_all(&self, agent_name: &str) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| is_addressed_to(m, agent_name))
            .collect()
    }

    /// Mark messages as read for `agent_name`.
    pub fn mark_read(&mut self, agent_name: &str, message_ids: &[String]) {
        let read = self
            .read_state
            .entry(agent_name.to_string())
            .or_default();
        for id in message_ids {
            read.insert(id.clone());
        }
    }

    /// Returns all messages between two agents.
    pub fn get_conversation(&self, agent1: &str, agent2: &str) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| {
                (m.from == agent1 && m.to == agent2) || (m.from == agent2 && m.to == agent1)
            })
            .collect()
    }

    /// Subscribe to messages for `agent_name`.
    pub fn subscribe<F>(&mut self, agent_name: &str, callback: F)
    where
        F: Fn(&Message) + Send + Sync + 'static,
    {
        self.subscribers
            .entry(agent_name.to_string())
            .or_default()
            .push(Box::new(callback));
    }

    fn persist(&mut self, message: Message) {
        self.messages.push(message.clone());
        self.notify_subscribers(&message);
    }

    fn notify_subscribers(&self, message: &Message) {
        if message.to != "*" {
            self.fire_callbacks(&message.to, message);
        } else {
            for (agent_name, _) in &self.subscribers {
                if agent_name != &message.from {
                    self.fire_callbacks(agent_name, message);
                }
            }
        }
    }

    fn fire_callbacks(&self, agent_name: &str, message: &Message) {
        if let Some(subs) = self.subscribers.get(agent_name) {
            for callback in subs {
                callback(message);
            }
        }
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

fn is_addressed_to(message: &Message, agent_name: &str) -> bool {
    if message.to == "*" {
        return message.from != agent_name;
    }
    message.to == agent_name
}
