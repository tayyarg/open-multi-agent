//! Dependency-aware task queue.
//!
//! [`TaskQueue`] owns the mutable lifecycle of every task it holds.
//! Completing a task automatically unblocks dependents.

use crate::task::task::is_task_ready;
use crate::types::{Task, TaskStatus};
use chrono::Utc;
use std::collections::HashMap;

/// Named event types emitted by [`TaskQueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskQueueEvent {
    TaskReady,
    TaskComplete,
    TaskFailed,
    AllComplete,
}

/// Mutable, event-driven queue with topological dependency resolution.
pub struct TaskQueue {
    tasks: HashMap<String, Task>,
    listeners: HashMap<TaskQueueEvent, Vec<Box<dyn Fn(Option<&Task>) + Send + Sync>>>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            listeners: HashMap::new(),
        }
    }

    /// Adds a single task.
    pub fn add(&mut self, task: Task) {
        let resolved = self.resolve_initial_status(task);
        let is_pending = resolved.status == TaskStatus::Pending;
        let id = resolved.id.clone();
        self.tasks.insert(id.clone(), resolved);
        if is_pending {
            if let Some(task) = self.tasks.get(&id) {
                self.emit(TaskQueueEvent::TaskReady, Some(task));
            }
        }
    }

    /// Adds multiple tasks at once.
    pub fn add_batch(&mut self, tasks: Vec<Task>) {
        for task in tasks {
            self.add(task);
        }
    }

    /// Applies a partial update to an existing task.
    pub fn update(
        &mut self,
        task_id: &str,
        status: Option<TaskStatus>,
        result: Option<String>,
        assignee: Option<String>,
    ) -> Result<Task, String> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("TaskQueue: task \"{}\" not found.", task_id))?;

        if let Some(s) = status {
            task.status = s;
        }
        if let Some(r) = result {
            task.result = Some(r);
        }
        if let Some(a) = assignee {
            task.assignee = Some(a);
        }
        task.updated_at = Utc::now();

        Ok(task.clone())
    }

    /// Marks a task as completed and unblocks dependents.
    pub fn complete(&mut self, task_id: &str, result: Option<String>) -> Result<Task, String> {
        let completed = self.update(task_id, Some(TaskStatus::Completed), result, None)?;
        self.emit(TaskQueueEvent::TaskComplete, Some(&completed));
        self.unblock_dependents(task_id);
        if self.is_complete() {
            self.emit(TaskQueueEvent::AllComplete, None);
        }
        Ok(completed)
    }

    /// Marks a task as failed and cascades failure to dependents.
    pub fn fail(&mut self, task_id: &str, error: String) -> Result<Task, String> {
        let failed = self.update(task_id, Some(TaskStatus::Failed), Some(error), None)?;
        self.emit(TaskQueueEvent::TaskFailed, Some(&failed));
        self.cascade_failure(task_id);
        if self.is_complete() {
            self.emit(TaskQueueEvent::AllComplete, None);
        }
        Ok(failed)
    }

    /// Returns the next pending task for an assignee.
    pub fn next(&self, assignee: Option<&str>) -> Option<&Task> {
        match assignee {
            Some(name) => self
                .tasks
                .values()
                .find(|t| t.status == TaskStatus::Pending && t.assignee.as_deref() == Some(name)),
            None => self.next_available(),
        }
    }

    /// Returns the next available pending task.
    pub fn next_available(&self) -> Option<&Task> {
        let mut fallback = None;
        for task in self.tasks.values() {
            if task.status != TaskStatus::Pending {
                continue;
            }
            if task.assignee.is_none() {
                return Some(task);
            }
            if fallback.is_none() {
                fallback = Some(task);
            }
        }
        fallback
    }

    /// Returns a snapshot of all tasks.
    pub fn list(&self) -> Vec<Task> {
        self.tasks.values().cloned().collect()
    }

    /// Returns all tasks with a given status.
    pub fn get_by_status(&self, status: TaskStatus) -> Vec<Task> {
        self.tasks
            .values()
            .filter(|t| t.status == status)
            .cloned()
            .collect()
    }

    /// Returns true when every task has reached a terminal state.
    pub fn is_complete(&self) -> bool {
        self.tasks.values().all(|t| {
            t.status == TaskStatus::Completed || t.status == TaskStatus::Failed
        })
    }

    /// Returns a progress snapshot.
    pub fn get_progress(&self) -> TaskProgress {
        let mut progress = TaskProgress::default();
        for task in self.tasks.values() {
            progress.total += 1;
            match task.status {
                TaskStatus::Completed => progress.completed += 1,
                TaskStatus::Failed => progress.failed += 1,
                TaskStatus::InProgress => progress.in_progress += 1,
                TaskStatus::Pending => progress.pending += 1,
                TaskStatus::Blocked => progress.blocked += 1,
            }
        }
        progress
    }

    /// Subscribe to a queue event.
    pub fn on<F>(&mut self, event: TaskQueueEvent, handler: F)
    where
        F: Fn(Option<&Task>) + Send + Sync + 'static,
    {
        self.listeners
            .entry(event)
            .or_default()
            .push(Box::new(handler));
    }

    // --- Private helpers ---

    fn resolve_initial_status(&self, task: Task) -> Task {
        if task.depends_on.is_none() || task.depends_on.as_ref().map_or(true, |d| d.is_empty()) {
            return task;
        }

        let all_current: Vec<Task> = self.tasks.values().cloned().collect();
        if is_task_ready(&task, &all_current, None) {
            return task;
        }

        Task {
            status: TaskStatus::Blocked,
            updated_at: Utc::now(),
            ..task
        }
    }

    fn unblock_dependents(&mut self, completed_id: &str) {
        let all_tasks: Vec<Task> = self.tasks.values().cloned().collect();
        let task_by_id: HashMap<String, &Task> = all_tasks.iter().map(|t| (t.id.clone(), t)).collect();

        let mut to_unblock = Vec::new();

        for task in &all_tasks {
            if task.status != TaskStatus::Blocked {
                continue;
            }
            if let Some(deps) = &task.depends_on {
                if !deps.contains(&completed_id.to_string()) {
                    continue;
                }
            } else {
                continue;
            }

            // Re-check with current state
            let ref_map: HashMap<String, &Task> = task_by_id.iter().map(|(k, v)| (k.clone(), *v)).collect();
            if is_task_ready(task, &all_tasks, Some(&ref_map)) {
                to_unblock.push(task.id.clone());
            }
        }

        for id in to_unblock {
            if let Some(task) = self.tasks.get_mut(&id) {
                task.status = TaskStatus::Pending;
                task.updated_at = Utc::now();
            }
            // Clone before emitting to avoid borrow issues
            if let Some(task) = self.tasks.get(&id) {
                self.emit(TaskQueueEvent::TaskReady, Some(task));
            }
        }
    }

    fn cascade_failure(&mut self, failed_task_id: &str) {
        let affected: Vec<String> = self
            .tasks
            .values()
            .filter(|t| {
                (t.status == TaskStatus::Blocked || t.status == TaskStatus::Pending)
                    && t.depends_on
                        .as_ref()
                        .map_or(false, |deps| deps.contains(&failed_task_id.to_string()))
            })
            .map(|t| t.id.clone())
            .collect();

        for id in affected {
            if let Some(task) = self.tasks.get_mut(&id) {
                task.status = TaskStatus::Failed;
                task.result = Some(format!(
                    "Cancelled: dependency \"{}\" failed.",
                    failed_task_id
                ));
                task.updated_at = Utc::now();
            }
            if let Some(task) = self.tasks.get(&id).cloned() {
                self.emit(TaskQueueEvent::TaskFailed, Some(&task));
                self.cascade_failure(&task.id);
            }
        }
    }

    fn emit(&self, event: TaskQueueEvent, task: Option<&Task>) {
        if let Some(handlers) = self.listeners.get(&event) {
            for handler in handlers {
                handler(task);
            }
        }
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TaskProgress {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub in_progress: usize,
    pub pending: usize,
    pub blocked: usize,
}

