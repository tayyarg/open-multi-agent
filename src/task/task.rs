//! Pure task utility functions.
//!
//! These helpers operate on plain [`Task`] values without any mutable
//! state, making them safe to use in reducers, tests, and reactive pipelines.

use crate::types::{Task, TaskStatus};
use chrono::Utc;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

/// Creates a new [`Task`] with a generated UUID, `Pending` status, and
/// timestamps set to the current instant.
pub fn create_task(
    title: &str,
    description: &str,
    assignee: Option<&str>,
    depends_on: Option<Vec<String>>,
) -> Task {
    let now = Utc::now();
    Task {
        id: Uuid::new_v4().to_string(),
        title: title.to_string(),
        description: description.to_string(),
        status: TaskStatus::Pending,
        assignee: assignee.map(String::from),
        depends_on,
        result: None,
        created_at: now,
        updated_at: now,
    }
}

/// Returns `true` when `task` can be started immediately.
///
/// A task is considered ready when:
/// 1. Its status is `Pending`.
/// 2. Every task listed in `depends_on` has status `Completed`.
pub fn is_task_ready(
    task: &Task,
    all_tasks: &[Task],
    task_by_id: Option<&HashMap<String, &Task>>,
) -> bool {
    if task.status != TaskStatus::Pending {
        return false;
    }

    let deps = match &task.depends_on {
        Some(d) if !d.is_empty() => d,
        _ => return true,
    };

    if let Some(map) = task_by_id {
        for dep_id in deps {
            match map.get(dep_id) {
                Some(dep) if dep.status == TaskStatus::Completed => {}
                _ => return false,
            }
        }
    } else {
        let map: HashMap<String, &Task> = all_tasks.iter().map(|t| (t.id.clone(), t)).collect();
        for dep_id in deps {
            match map.get(dep_id) {
                Some(dep) if dep.status == TaskStatus::Completed => {}
                _ => return false,
            }
        }
    }

    true
}

/// Returns `tasks` sorted so that each task appears after all of its
/// dependencies (Kahn's algorithm).
pub fn get_task_dependency_order(tasks: &[Task]) -> Vec<Task> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let task_by_id: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut successors: HashMap<&str, Vec<&str>> = HashMap::new();

    for task in tasks {
        in_degree.entry(task.id.as_str()).or_insert(0);
        successors.entry(task.id.as_str()).or_default();

        if let Some(deps) = &task.depends_on {
            for dep_id in deps {
                if task_by_id.contains_key(dep_id.as_str()) {
                    *in_degree.entry(task.id.as_str()).or_insert(0) += 1;
                    successors
                        .entry(dep_id.as_str())
                        .or_default()
                        .push(task.id.as_str());
                }
            }
        }
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&id, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(id);
        }
    }

    let mut ordered = Vec::new();
    while let Some(id) = queue.pop_front() {
        if let Some(task) = task_by_id.get(id) {
            ordered.push((*task).clone());
        }

        if let Some(succs) = successors.get(id) {
            for &succ_id in succs {
                if let Some(degree) = in_degree.get_mut(succ_id) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(succ_id);
                    }
                }
            }
        }
    }

    ordered
}

/// Validates the dependency graph of a task collection.
pub fn validate_task_dependencies(tasks: &[Task]) -> (bool, Vec<String>) {
    let mut errors = Vec::new();
    let task_ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();

    // Pass 1: unknown references and self-dependencies
    for task in tasks {
        if let Some(deps) = &task.depends_on {
            for dep_id in deps {
                if dep_id == &task.id {
                    errors.push(format!(
                        "Task \"{}\" ({}) depends on itself.",
                        task.title, task.id
                    ));
                    continue;
                }
                if !task_ids.contains(dep_id.as_str()) {
                    errors.push(format!(
                        "Task \"{}\" ({}) references unknown dependency \"{}\".",
                        task.title, task.id, dep_id
                    ));
                }
            }
        }
    }

    // Pass 2: cycle detection via DFS colouring
    let task_by_id: HashMap<&str, &Task> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut colour: HashMap<&str, u8> = tasks.iter().map(|t| (t.id.as_str(), 0u8)).collect();

    fn visit<'a>(
        id: &'a str,
        path: &mut Vec<&'a str>,
        task_by_id: &HashMap<&'a str, &'a Task>,
        colour: &mut HashMap<&'a str, u8>,
        errors: &mut Vec<String>,
    ) {
        if colour.get(id) == Some(&2) {
            return;
        }
        if colour.get(id) == Some(&1) {
            if let Some(pos) = path.iter().position(|&p| p == id) {
                let cycle: Vec<&str> = path[pos..].to_vec();
                let mut cycle_str: Vec<String> = cycle.iter().map(|s| s.to_string()).collect();
                cycle_str.push(id.to_string());
                errors.push(format!("Cyclic dependency detected: {}", cycle_str.join(" -> ")));
            }
            return;
        }

        colour.insert(id, 1);
        path.push(id);

        if let Some(task) = task_by_id.get(id) {
            if let Some(deps) = &task.depends_on {
                for dep_id in deps {
                    if task_by_id.contains_key(dep_id.as_str()) {
                        visit(dep_id, path, task_by_id, colour, errors);
                    }
                }
            }
        }

        path.pop();
        colour.insert(id, 2);
    }

    for task in tasks {
        if colour.get(task.id.as_str()) == Some(&0) {
            let mut path = Vec::new();
            visit(task.id.as_str(), &mut path, &task_by_id, &mut colour, &mut errors);
        }
    }

    (errors.is_empty(), errors)
}
