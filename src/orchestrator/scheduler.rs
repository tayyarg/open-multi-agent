//! Task scheduling strategies for the open-multi-agent orchestrator.
//!
//! The [`Scheduler`] class encapsulates four distinct strategies for
//! mapping a set of pending [`Task`]s onto a pool of available agents:
//!
//! - `round-robin`        -- Distribute tasks evenly across agents by index.
//! - `least-busy`         -- Assign to whichever agent has the fewest active tasks.
//! - `capability-match`   -- Score agents by keyword overlap with the task description.
//! - `dependency-first`   -- Prioritise tasks on the critical path (most blocked dependents).
//!
//! The scheduler is stateless between calls. All mutable task state lives in the
//! [`TaskQueue`] that is passed to [`Scheduler::auto_assign`].

use crate::task::queue::TaskQueue;
use crate::types::{AgentConfig, Task, TaskStatus};
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The four scheduling strategies available to the [`Scheduler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingStrategy {
    RoundRobin,
    LeastBusy,
    CapabilityMatch,
    DependencyFirst,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Count how many tasks in `all_tasks` are (transitively) blocked waiting for
/// `task_id` to complete. Used by the `dependency-first` strategy to compute
/// the "criticality" of each pending task.
fn count_blocked_dependents(task_id: &str, all_tasks: &[Task]) -> usize {
    // Build reverse adjacency: dependency_id -> tasks that depend on it
    let id_set: HashSet<&str> = all_tasks.iter().map(|t| t.id.as_str()).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for t in all_tasks {
        if let Some(deps) = &t.depends_on {
            for dep_id in deps {
                dependents
                    .entry(dep_id.as_str())
                    .or_default()
                    .push(t.id.as_str());
            }
        }
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    queue.push_back(task_id);

    while let Some(current) = queue.pop_front() {
        if let Some(deps) = dependents.get(current) {
            for &dep_id in deps {
                if !visited.contains(dep_id) && id_set.contains(dep_id) {
                    visited.insert(dep_id);
                    queue.push_back(dep_id);
                }
            }
        }
    }

    visited.len()
}

/// Compute a simple keyword-overlap score between `text` and `keywords`.
fn keyword_score(text: &str, keywords: &[String]) -> usize {
    let lower = text.to_lowercase();
    keywords
        .iter()
        .filter(|kw| lower.contains(&kw.to_lowercase()))
        .count()
}

/// Extract a list of meaningful keywords from a string for capability matching.
fn extract_keywords(text: &str) -> Vec<String> {
    let stop_words: HashSet<&str> = [
        "the", "and", "for", "that", "this", "with", "are", "from", "have",
        "will", "your", "you", "can", "all", "each", "when", "then", "they",
        "them", "their", "about", "into", "more", "also", "should", "must",
    ]
    .into_iter()
    .collect();

    let mut seen = HashSet::new();
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !stop_words.contains(w))
        .filter(|w| seen.insert(w.to_string()))
        .map(String::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Maps pending tasks to available agents using one of four configurable strategies.
pub struct Scheduler {
    strategy: SchedulingStrategy,
    round_robin_cursor: usize,
}

impl Scheduler {
    pub fn new(strategy: SchedulingStrategy) -> Self {
        Self {
            strategy,
            round_robin_cursor: 0,
        }
    }

    /// Given a list of pending `tasks` and `agents`, return a mapping from
    /// `task_id` to `agent_name` representing the recommended assignment.
    pub fn schedule(
        &mut self,
        tasks: &[Task],
        agents: &[AgentConfig],
    ) -> HashMap<String, String> {
        if agents.is_empty() {
            return HashMap::new();
        }

        let unassigned: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending && t.assignee.is_none())
            .collect();

        match self.strategy {
            SchedulingStrategy::RoundRobin => self.schedule_round_robin(&unassigned, agents),
            SchedulingStrategy::LeastBusy => {
                self.schedule_least_busy(&unassigned, agents, tasks)
            }
            SchedulingStrategy::CapabilityMatch => {
                self.schedule_capability_match(&unassigned, agents)
            }
            SchedulingStrategy::DependencyFirst => {
                self.schedule_dependency_first(&unassigned, agents, tasks)
            }
        }
    }

    /// Convenience method that applies assignments returned by [`schedule`]
    /// directly to a live `TaskQueue`.
    pub fn auto_assign(&mut self, queue: &mut TaskQueue, agents: &[AgentConfig]) {
        let all_tasks = queue.list();
        let assignments = self.schedule(&all_tasks, agents);

        for (task_id, agent_name) in assignments {
            let _ = queue.update(&task_id, None, None, Some(agent_name));
        }
    }

    // -------------------------------------------------------------------------
    // Strategy implementations
    // -------------------------------------------------------------------------

    fn schedule_round_robin(
        &mut self,
        unassigned: &[&Task],
        agents: &[AgentConfig],
    ) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for task in unassigned {
            let agent = &agents[self.round_robin_cursor % agents.len()];
            result.insert(task.id.clone(), agent.name.clone());
            self.round_robin_cursor = (self.round_robin_cursor + 1) % agents.len();
        }
        result
    }

    fn schedule_least_busy(
        &self,
        unassigned: &[&Task],
        agents: &[AgentConfig],
        all_tasks: &[Task],
    ) -> HashMap<String, String> {
        let mut load: HashMap<&str, usize> = agents.iter().map(|a| (a.name.as_str(), 0)).collect();
        for task in all_tasks {
            if task.status == TaskStatus::InProgress {
                if let Some(assignee) = &task.assignee {
                    if let Some(count) = load.get_mut(assignee.as_str()) {
                        *count += 1;
                    }
                }
            }
        }

        let mut result = HashMap::new();
        for task in unassigned {
            let best_agent = agents
                .iter()
                .min_by_key(|a| load.get(a.name.as_str()).copied().unwrap_or(0))
                .unwrap();

            result.insert(task.id.clone(), best_agent.name.clone());
            *load.get_mut(best_agent.name.as_str()).unwrap() += 1;
        }

        result
    }

    fn schedule_capability_match(
        &self,
        unassigned: &[&Task],
        agents: &[AgentConfig],
    ) -> HashMap<String, String> {
        let agent_keywords: HashMap<&str, Vec<String>> = agents
            .iter()
            .map(|a| {
                let text = format!(
                    "{} {} {}",
                    a.name,
                    a.system_prompt.as_deref().unwrap_or(""),
                    a.model
                );
                (a.name.as_str(), extract_keywords(&text))
            })
            .collect();

        let mut result = HashMap::new();
        for task in unassigned {
            let task_text = format!("{} {}", task.title, task.description);
            let task_kws = extract_keywords(&task_text);

            let best_agent = agents
                .iter()
                .max_by_key(|a| {
                    let agent_text = format!(
                        "{} {}",
                        a.name,
                        a.system_prompt.as_deref().unwrap_or("")
                    );
                    let score_a = keyword_score(&agent_text, &task_kws);
                    let score_b = keyword_score(
                        &task_text,
                        agent_keywords.get(a.name.as_str()).unwrap(),
                    );
                    score_a + score_b
                })
                .unwrap();

            result.insert(task.id.clone(), best_agent.name.clone());
        }

        result
    }

    fn schedule_dependency_first(
        &mut self,
        unassigned: &[&Task],
        agents: &[AgentConfig],
        all_tasks: &[Task],
    ) -> HashMap<String, String> {
        let mut ranked: Vec<&Task> = unassigned.to_vec();
        ranked.sort_by(|a, b| {
            let crit_a = count_blocked_dependents(&a.id, all_tasks);
            let crit_b = count_blocked_dependents(&b.id, all_tasks);
            crit_b.cmp(&crit_a)
        });

        let mut result = HashMap::new();
        let mut cursor = self.round_robin_cursor;

        for task in ranked {
            let agent = &agents[cursor % agents.len()];
            result.insert(task.id.clone(), agent.name.clone());
            cursor = (cursor + 1) % agents.len();
        }

        self.round_robin_cursor = cursor;
        result
    }
}
