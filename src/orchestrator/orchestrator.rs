//! OpenMultiAgent -- the top-level multi-agent orchestration class.
//!
//! [`OpenMultiAgent`] is the primary public API of the open-multi-agent framework.
//! It ties together every subsystem:
//!
//!  - [`Team`]       -- Agent roster, shared memory, inter-agent messaging
//!  - [`TaskQueue`]  -- Dependency-aware work queue
//!  - [`Scheduler`]  -- Task-to-agent assignment strategies
//!  - [`AgentPool`]  -- Concurrency-controlled execution pool
//!  - [`Agent`]      -- Conversation + tool-execution loop

use crate::agent::agent::Agent;
use crate::agent::pool::AgentPool;
use crate::orchestrator::scheduler::{Scheduler, SchedulingStrategy};
use crate::task::queue::TaskQueue;
use crate::task::task::create_task;
use crate::team::team::Team;
use crate::tool::built_in::register_built_in_tools;
use crate::tool::executor::ToolExecutor;
use crate::tool::framework::ToolRegistry;
use crate::types::*;
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Internal constants
// ---------------------------------------------------------------------------

const DEFAULT_MAX_CONCURRENCY: usize = 5;
const DEFAULT_MODEL: &str = "claude-opus-4-6";

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a minimal [`Agent`] with its own fresh registry/executor.
/// Registers all built-in tools so coordinator/worker agents can use them.
fn build_agent(config: AgentConfig) -> Arc<Agent> {
    let mut registry = ToolRegistry::new();
    register_built_in_tools(&mut registry);
    let registry = Arc::new(registry);
    let executor = Arc::new(ToolExecutor::new(registry.clone(), None));
    Arc::new(Agent::new(config, registry, executor))
}

// ---------------------------------------------------------------------------
// Parsed task spec (result of coordinator decomposition)
// ---------------------------------------------------------------------------

struct ParsedTaskSpec {
    title: String,
    description: String,
    assignee: Option<String>,
    depends_on: Option<Vec<String>>,
}

/// Attempt to extract a JSON array of task specs from the coordinator's raw
/// output. The coordinator is prompted to emit JSON inside a ```json ... ```
/// fence or as a bare array.
fn parse_task_specs(raw: &str) -> Option<Vec<ParsedTaskSpec>> {
    // Strategy 1: look for a fenced JSON block
    let candidate = if let Some(caps) = regex::Regex::new(r"```json\s*([\s\S]*?)```")
        .ok()
        .and_then(|re| re.captures(raw))
    {
        caps.get(1).map_or(raw, |m| m.as_str()).to_string()
    } else {
        raw.to_string()
    };

    // Strategy 2: find the first '[' and last ']'
    let array_start = candidate.find('[')?;
    let array_end = candidate.rfind(']')?;
    if array_end <= array_start {
        return None;
    }

    let json_slice = &candidate[array_start..=array_end];
    let parsed: serde_json::Value = serde_json::from_str(json_slice).ok()?;
    let arr = parsed.as_array()?;

    let mut specs = Vec::new();
    for item in arr {
        let obj = item.as_object()?;
        let title = obj.get("title")?.as_str()?.to_string();
        let description = obj.get("description")?.as_str()?.to_string();

        let assignee = obj
            .get("assignee")
            .and_then(|v| v.as_str())
            .map(String::from);

        let depends_on = obj.get("dependsOn").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
        });

        specs.push(ParsedTaskSpec {
            title,
            description,
            assignee,
            depends_on,
        });
    }

    if specs.is_empty() {
        None
    } else {
        Some(specs)
    }
}

// ---------------------------------------------------------------------------
// Orchestration loop
// ---------------------------------------------------------------------------

/// Internal execution context assembled once per `run_team` / `run_tasks` call.
struct RunContext<'a> {
    team: &'a Team,
    pool: &'a AgentPool,
    scheduler: &'a mut Scheduler,
    agent_results: &'a mut HashMap<String, AgentRunResult>,
    on_progress: &'a Option<Arc<dyn Fn(OrchestratorEvent) + Send + Sync>>,
}

/// Execute all tasks in `queue` using agents in `pool`, respecting dependencies
/// and running independent tasks in parallel.
async fn execute_queue(queue: &mut TaskQueue, ctx: &mut RunContext<'_>) {
    let agents = ctx.team.get_agents();

    loop {
        ctx.scheduler.auto_assign(queue, &agents);

        let pending = queue.get_by_status(TaskStatus::Pending);
        if pending.is_empty() {
            break;
        }

        // Dispatch all currently-pending tasks as a parallel batch via tokio::spawn.
        let mut handles = Vec::new();

        for task in &pending {
            // Mark in-progress
            let _ = queue.update(&task.id, Some(TaskStatus::InProgress), None, None);

            let assignee = match &task.assignee {
                Some(a) => a.clone(),
                None => {
                    let msg = format!("Task \"{}\" has no assignee.", task.title);
                    let _ = queue.fail(&task.id, msg.clone());
                    if let Some(cb) = ctx.on_progress.as_ref() {
                        cb(OrchestratorEvent {
                            event_type: OrchestratorEventType::Error,
                            task: Some(task.id.clone()),
                            agent: None,
                            data: Some(serde_json::Value::String(msg)),
                        });
                    }
                    continue;
                }
            };

            let agent = match ctx.pool.get(&assignee) {
                Some(a) => a,
                None => {
                    let msg = format!(
                        "Agent \"{}\" not found in pool for task \"{}\".",
                        assignee, task.title
                    );
                    let _ = queue.fail(&task.id, msg.clone());
                    if let Some(cb) = ctx.on_progress.as_ref() {
                        cb(OrchestratorEvent {
                            event_type: OrchestratorEventType::Error,
                            task: Some(task.id.clone()),
                            agent: Some(assignee),
                            data: Some(serde_json::Value::String(msg)),
                        });
                    }
                    continue;
                }
            };

            if let Some(cb) = ctx.on_progress.as_ref() {
                cb(OrchestratorEvent {
                    event_type: OrchestratorEventType::TaskStart,
                    task: Some(task.id.clone()),
                    agent: Some(assignee.clone()),
                    data: None,
                });
                cb(OrchestratorEvent {
                    event_type: OrchestratorEventType::AgentStart,
                    agent: Some(assignee.clone()),
                    task: Some(task.id.clone()),
                    data: None,
                });
            }

            let prompt = build_task_prompt(task, ctx.team).await;
            let task_id = task.id.clone();
            let agent_name = assignee.clone();

            let handle = tokio::spawn(async move {
                let result = agent.run(&prompt).await;
                (task_id, agent_name, result)
            });
            handles.push(handle);
        }

        // Wait for the entire parallel batch before checking for newly-unblocked tasks.
        for handle in handles {
            if let Ok((task_id, agent_name, result)) = handle.await {
                let key = format!("{}:{}", agent_name, task_id);
                ctx.agent_results.insert(key, result.clone());

                if result.success {
                    // Persist result into shared memory so other agents can read it
                    if let Some(shared_mem) = ctx.team.get_shared_memory_instance() {
                        shared_mem
                            .write(
                                &agent_name,
                                &format!("task:{}:result", task_id),
                                &result.output,
                                None,
                            )
                            .await;
                    }

                    let _ = queue.complete(&task_id, Some(result.output.clone()));

                    if let Some(cb) = ctx.on_progress.as_ref() {
                        cb(OrchestratorEvent {
                            event_type: OrchestratorEventType::TaskComplete,
                            task: Some(task_id.clone()),
                            agent: Some(agent_name.clone()),
                            data: None,
                        });
                        cb(OrchestratorEvent {
                            event_type: OrchestratorEventType::AgentComplete,
                            agent: Some(agent_name),
                            task: Some(task_id),
                            data: None,
                        });
                    }
                } else {
                    let _ = queue.fail(&task_id, result.output.clone());
                    if let Some(cb) = ctx.on_progress.as_ref() {
                        cb(OrchestratorEvent {
                            event_type: OrchestratorEventType::Error,
                            task: Some(task_id),
                            agent: Some(agent_name),
                            data: Some(serde_json::Value::String(result.output)),
                        });
                    }
                }
            }
        }
    }
}

/// Build the agent prompt for a specific task.
async fn build_task_prompt(task: &Task, team: &Team) -> String {
    let mut lines = vec![
        format!("# Task: {}", task.title),
        String::new(),
        task.description.clone(),
    ];

    // Inject shared memory summary so the agent sees its teammates' work
    if let Some(shared_mem) = team.get_shared_memory_instance() {
        let summary = shared_mem.get_summary().await;
        if !summary.is_empty() {
            lines.push(String::new());
            lines.push(summary);
        }
    }

    // Inject messages from other agents addressed to this assignee
    if let Some(assignee) = &task.assignee {
        let messages = team.get_messages(assignee);
        if !messages.is_empty() {
            lines.push(String::new());
            lines.push("## Messages from team members".to_string());
            for msg in &messages {
                lines.push(format!("- **{}**: {}", msg.from, msg.content));
            }
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// OpenMultiAgent
// ---------------------------------------------------------------------------

/// Top-level orchestrator for the open-multi-agent framework.
///
/// Manages teams, coordinates task execution, and surfaces progress events.
/// Most users will interact with this struct exclusively.
pub struct OpenMultiAgent {
    max_concurrency: usize,
    default_model: String,
    default_provider: Provider,
    default_base_url: Option<String>,
    default_api_key: Option<String>,
    on_progress: Option<Arc<dyn Fn(OrchestratorEvent) + Send + Sync>>,
    teams: HashMap<String, Team>,
    completed_task_count: usize,
}

impl OpenMultiAgent {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            max_concurrency: config.max_concurrency.unwrap_or(DEFAULT_MAX_CONCURRENCY),
            default_model: config
                .default_model
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            default_provider: config.default_provider.unwrap_or(Provider::Anthropic),
            default_base_url: config.default_base_url,
            default_api_key: config.default_api_key,
            on_progress: config.on_progress.map(Arc::from),
            teams: HashMap::new(),
            completed_task_count: 0,
        }
    }

    // -------------------------------------------------------------------------
    // Team management
    // -------------------------------------------------------------------------

    /// Create and register a [`Team`] with the orchestrator.
    pub fn create_team(&mut self, name: &str, config: TeamConfig) -> &Team {
        if self.teams.contains_key(name) {
            panic!(
                "OpenMultiAgent: a team named \"{}\" already exists. \
                 Use a unique name or call shutdown() to clear all teams.",
                name
            );
        }
        let team = Team::new(config);
        self.teams.insert(name.to_string(), team);
        self.teams.get(name).unwrap()
    }

    // -------------------------------------------------------------------------
    // Single-agent convenience
    // -------------------------------------------------------------------------

    /// Run a single prompt with a one-off agent.
    pub async fn run_agent(
        &mut self,
        config: AgentConfig,
        prompt: &str,
    ) -> AgentRunResult {
        let effective = AgentConfig {
            provider: config.provider.or(Some(self.default_provider)),
            base_url: config.base_url.or_else(|| self.default_base_url.clone()),
            api_key: config.api_key.or_else(|| self.default_api_key.clone()),
            ..config
        };

        let agent = build_agent(effective);

        if let Some(cb) = &self.on_progress {
            cb(OrchestratorEvent {
                event_type: OrchestratorEventType::AgentStart,
                agent: Some(agent.name.clone()),
                task: None,
                data: None,
            });
        }

        let result = agent.run(prompt).await;

        if let Some(cb) = &self.on_progress {
            cb(OrchestratorEvent {
                event_type: OrchestratorEventType::AgentComplete,
                agent: Some(agent.name.clone()),
                task: None,
                data: None,
            });
        }

        if result.success {
            self.completed_task_count += 1;
        }

        result
    }

    // -------------------------------------------------------------------------
    // Auto-orchestrated team run (KILLER FEATURE)
    // -------------------------------------------------------------------------

    /// Run a team on a high-level goal with full automatic orchestration.
    ///
    /// 1. A temporary "coordinator" agent decomposes the goal into tasks.
    /// 2. Tasks are loaded into a [`TaskQueue`] with dependency resolution.
    /// 3. The [`Scheduler`] assigns unassigned tasks to team agents.
    /// 4. Tasks execute in dependency order, with independent tasks in parallel.
    /// 5. Results are persisted to shared memory after each task.
    /// 6. The coordinator synthesises a final answer from all task outputs.
    /// 7. A [`TeamRunResult`] is returned.
    pub async fn run_team(&mut self, team_name: &str, goal: &str) -> Result<TeamRunResult, String> {
        let team = self
            .teams
            .get(team_name)
            .ok_or_else(|| format!("Team \"{}\" not found.", team_name))?;

        let agent_configs = team.get_agents();

        // Step 1: Coordinator decomposes goal into tasks
        let coordinator_config = AgentConfig {
            name: "coordinator".to_string(),
            model: self.default_model.clone(),
            provider: Some(self.default_provider),
            base_url: self.default_base_url.clone(),
            api_key: self.default_api_key.clone(),
            system_prompt: Some(Self::build_coordinator_system_prompt(&agent_configs)),
            tools: Vec::new(),
            max_turns: Some(3),
            max_tokens: None,
            temperature: None,
        };

        let decomposition_prompt =
            Self::build_decomposition_prompt(goal, &agent_configs);
        let coordinator_agent = build_agent(coordinator_config);

        if let Some(cb) = &self.on_progress {
            cb(OrchestratorEvent {
                event_type: OrchestratorEventType::AgentStart,
                agent: Some("coordinator".to_string()),
                task: None,
                data: None,
            });
        }

        let decomposition_result = coordinator_agent.run(&decomposition_prompt).await;
        let mut agent_results = HashMap::new();
        agent_results.insert("coordinator:decompose".to_string(), decomposition_result.clone());

        // Step 2: Parse tasks from coordinator output
        let task_specs = parse_task_specs(&decomposition_result.output);

        let mut queue = TaskQueue::new();
        let mut scheduler = Scheduler::new(SchedulingStrategy::DependencyFirst);

        if let Some(specs) = task_specs {
            if !specs.is_empty() {
                Self::load_specs_into_queue(&specs, &agent_configs, &mut queue);
            } else {
                Self::fallback_tasks(goal, &agent_configs, &mut queue);
            }
        } else {
            Self::fallback_tasks(goal, &agent_configs, &mut queue);
        }

        // Step 3: Auto-assign any unassigned tasks
        scheduler.auto_assign(&mut queue, &agent_configs);

        // Step 4: Build pool and execute
        let pool = self.build_pool(&agent_configs);

        // We need to get the team reference again after the mutable borrow above
        let team = self.teams.get(team_name).unwrap();

        {
            let mut ctx = RunContext {
                team,
                pool: &pool,
                scheduler: &mut scheduler,
                agent_results: &mut agent_results,
                on_progress: &self.on_progress,
            };
            execute_queue(&mut queue, &mut ctx).await;
        }

        // Step 5: Coordinator synthesises final result
        let team = self.teams.get(team_name).unwrap();
        let synthesis_prompt =
            Self::build_synthesis_prompt(goal, &queue.list(), team).await;
        let synthesis_result = coordinator_agent.run(&synthesis_prompt).await;
        agent_results.insert("coordinator".to_string(), synthesis_result);

        if let Some(cb) = &self.on_progress {
            cb(OrchestratorEvent {
                event_type: OrchestratorEventType::AgentComplete,
                agent: Some("coordinator".to_string()),
                task: None,
                data: None,
            });
        }

        Ok(self.build_team_run_result(&agent_results))
    }

    // -------------------------------------------------------------------------
    // Explicit-task team run
    // -------------------------------------------------------------------------

    /// Run a team with an explicitly provided task list.
    pub async fn run_tasks(
        &mut self,
        team_name: &str,
        tasks: Vec<ParsedTaskSpecInput>,
    ) -> Result<TeamRunResult, String> {
        let team = self
            .teams
            .get(team_name)
            .ok_or_else(|| format!("Team \"{}\" not found.", team_name))?;

        let agent_configs = team.get_agents();
        let mut queue = TaskQueue::new();
        let mut scheduler = Scheduler::new(SchedulingStrategy::DependencyFirst);

        let specs: Vec<ParsedTaskSpec> = tasks
            .into_iter()
            .map(|t| ParsedTaskSpec {
                title: t.title,
                description: t.description,
                assignee: t.assignee,
                depends_on: t.depends_on,
            })
            .collect();

        Self::load_specs_into_queue(&specs, &agent_configs, &mut queue);
        scheduler.auto_assign(&mut queue, &agent_configs);

        let pool = self.build_pool(&agent_configs);
        let mut agent_results = HashMap::new();

        let team = self.teams.get(team_name).unwrap();
        {
            let mut ctx = RunContext {
                team,
                pool: &pool,
                scheduler: &mut scheduler,
                agent_results: &mut agent_results,
                on_progress: &self.on_progress,
            };
            execute_queue(&mut queue, &mut ctx).await;
        }

        Ok(self.build_team_run_result(&agent_results))
    }

    // -------------------------------------------------------------------------
    // Observability
    // -------------------------------------------------------------------------

    /// Returns a lightweight status snapshot.
    pub fn get_status(&self) -> OrchestratorStatus {
        OrchestratorStatus {
            teams: self.teams.len(),
            active_agents: 0, // Pools are ephemeral per-run
            completed_tasks: self.completed_task_count,
        }
    }

    // -------------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------------

    /// Deregister all teams and reset internal counters.
    pub async fn shutdown(&mut self) {
        self.teams.clear();
        self.completed_task_count = 0;
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    fn build_coordinator_system_prompt(agents: &[AgentConfig]) -> String {
        let roster: Vec<String> = agents
            .iter()
            .map(|a| {
                let desc = a
                    .system_prompt
                    .as_ref()
                    .map(|s| s.chars().take(120).collect::<String>())
                    .unwrap_or_else(|| "general purpose agent".to_string());
                format!("- **{}** ({}): {}", a.name, a.model, desc)
            })
            .collect();

        [
            "You are a task coordinator responsible for decomposing high-level goals",
            "into concrete, actionable tasks and assigning them to the right team members.",
            "",
            "## Team Roster",
            &roster.join("\n"),
            "",
            "## Output Format",
            "When asked to decompose a goal, respond ONLY with a JSON array of task objects.",
            "Each task must have:",
            "  - \"title\":       Short descriptive title (string)",
            "  - \"description\": Full task description with context and expected output (string)",
            "  - \"assignee\":    One of the agent names listed in the roster (string)",
            "  - \"dependsOn\":   Array of titles of tasks this task depends on (string[], may be empty)",
            "",
            "Wrap the JSON in a ```json code fence.",
            "Do not include any text outside the code fence.",
            "",
            "## When synthesising results",
            "You will be given completed task outputs and asked to synthesise a final answer.",
            "Write a clear, comprehensive response that addresses the original goal.",
        ]
        .join("\n")
    }

    fn build_decomposition_prompt(goal: &str, agents: &[AgentConfig]) -> String {
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        [
            &format!(
                "Decompose the following goal into tasks for your team ({}).",
                names.join(", ")
            ),
            "",
            "## Goal",
            goal,
            "",
            "Return ONLY the JSON task array in a ```json code fence.",
        ]
        .join("\n")
    }

    async fn build_synthesis_prompt(goal: &str, tasks: &[Task], team: &Team) -> String {
        let completed: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();
        let failed: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .collect();

        let result_sections: Vec<String> = completed
            .iter()
            .map(|t| {
                let assignee = t.assignee.as_deref().unwrap_or("unknown");
                format!(
                    "### {} (completed by {})\n{}",
                    t.title,
                    assignee,
                    t.result.as_deref().unwrap_or("(no output)")
                )
            })
            .collect();

        let failure_sections: Vec<String> = failed
            .iter()
            .map(|t| {
                format!(
                    "### {} (FAILED)\nError: {}",
                    t.title,
                    t.result.as_deref().unwrap_or("unknown error")
                )
            })
            .collect();

        let mut memory_summary = String::new();
        if let Some(shared_mem) = team.get_shared_memory_instance() {
            memory_summary = shared_mem.get_summary().await;
        }

        let mut lines = vec![
            "## Original Goal".to_string(),
            goal.to_string(),
            String::new(),
            "## Task Results".to_string(),
        ];
        lines.extend(result_sections);

        if !failure_sections.is_empty() {
            lines.push(String::new());
            lines.push("## Failed Tasks".to_string());
            lines.extend(failure_sections);
        }

        if !memory_summary.is_empty() {
            lines.push(String::new());
            lines.push(memory_summary);
        }

        lines.push(String::new());
        lines.push("## Your Task".to_string());
        lines.push(
            "Synthesise the above results into a comprehensive final answer that addresses the original goal.".to_string(),
        );
        lines.push("If some tasks failed, note any gaps in the result.".to_string());

        lines.join("\n")
    }

    /// Load a list of task specs into a queue, resolving title-based dependencies.
    fn load_specs_into_queue(
        specs: &[ParsedTaskSpec],
        agent_configs: &[AgentConfig],
        queue: &mut TaskQueue,
    ) {
        let agent_names: std::collections::HashSet<&str> =
            agent_configs.iter().map(|a| a.name.as_str()).collect();

        // First pass: create tasks (without dependencies) to get stable IDs.
        let mut title_to_id: HashMap<String, String> = HashMap::new();
        let mut created_tasks: Vec<Task> = Vec::new();

        for spec in specs {
            let assignee = spec
                .assignee
                .as_deref()
                .filter(|a| agent_names.contains(a));

            let task = create_task(&spec.title, &spec.description, assignee, None);
            title_to_id.insert(spec.title.to_lowercase().trim().to_string(), task.id.clone());
            created_tasks.push(task);
        }

        // Second pass: resolve title-based dependsOn to IDs.
        for (i, task) in created_tasks.into_iter().enumerate() {
            let spec = &specs[i];

            if spec.depends_on.is_none()
                || spec
                    .depends_on
                    .as_ref()
                    .map_or(true, |d| d.is_empty())
            {
                queue.add(task);
                continue;
            }

            let mut resolved_deps: Vec<String> = Vec::new();
            if let Some(dep_refs) = &spec.depends_on {
                for dep_ref in dep_refs {
                    // Accept both raw IDs and title strings
                    let by_title = title_to_id.get(&dep_ref.to_lowercase().trim().to_string());
                    if let Some(resolved_id) = by_title {
                        resolved_deps.push(resolved_id.clone());
                    }
                }
            }

            let task_with_deps = Task {
                depends_on: if resolved_deps.is_empty() {
                    None
                } else {
                    Some(resolved_deps)
                },
                ..task
            };
            queue.add(task_with_deps);
        }
    }

    /// Fallback: one task per agent using the goal as the description.
    fn fallback_tasks(
        goal: &str,
        agent_configs: &[AgentConfig],
        queue: &mut TaskQueue,
    ) {
        for agent_config in agent_configs {
            let title = format!(
                "{}: {}",
                agent_config.name,
                &goal[..goal.len().min(80)]
            );
            let task = create_task(&title, goal, Some(&agent_config.name), None);
            queue.add(task);
        }
    }

    /// Build an [`AgentPool`] from a list of agent configurations.
    fn build_pool(&self, agent_configs: &[AgentConfig]) -> AgentPool {
        let mut pool = AgentPool::new(self.max_concurrency);
        for config in agent_configs {
            let effective = AgentConfig {
                provider: config.provider.or(Some(self.default_provider)),
                base_url: config
                    .base_url
                    .clone()
                    .or_else(|| self.default_base_url.clone()),
                api_key: config
                    .api_key
                    .clone()
                    .or_else(|| self.default_api_key.clone()),
                ..config.clone()
            };
            pool.add(build_agent(effective));
        }
        pool
    }

    /// Aggregate the per-run agent_results map into a [`TeamRunResult`].
    fn build_team_run_result(
        &mut self,
        agent_results: &HashMap<String, AgentRunResult>,
    ) -> TeamRunResult {
        let mut total_usage = TokenUsage::default();
        let mut overall_success = true;
        let mut collapsed: HashMap<String, AgentRunResult> = HashMap::new();

        for (key, result) in agent_results {
            let agent_name = if key.contains(':') {
                key.split(':').next().unwrap_or(key).to_string()
            } else {
                key.clone()
            };

            total_usage = total_usage.add(&result.token_usage);
            if !result.success {
                overall_success = false;
            }

            if let Some(existing) = collapsed.get_mut(&agent_name) {
                existing.success = existing.success && result.success;
                if !existing.output.is_empty() && !result.output.is_empty() {
                    existing.output = format!("{}\n\n---\n\n{}", existing.output, result.output);
                } else if !result.output.is_empty() {
                    existing.output = result.output.clone();
                }
                existing.messages.extend(result.messages.clone());
                existing.token_usage = existing.token_usage.add(&result.token_usage);
                existing.tool_calls.extend(result.tool_calls.clone());
            } else {
                collapsed.insert(agent_name.clone(), result.clone());
            }

            // Only count actual user tasks -- skip coordinator meta-entries
            if result.success && !key.starts_with("coordinator") {
                self.completed_task_count += 1;
            }
        }

        TeamRunResult {
            success: overall_success,
            agent_results: collapsed,
            total_token_usage: total_usage,
        }
    }
}

// ---------------------------------------------------------------------------
// Public input type for run_tasks
// ---------------------------------------------------------------------------

/// Input descriptor for explicit task lists passed to [`OpenMultiAgent::run_tasks`].
pub struct ParsedTaskSpecInput {
    pub title: String,
    pub description: String,
    pub assignee: Option<String>,
    pub depends_on: Option<Vec<String>>,
}

/// Lightweight status snapshot.
#[derive(Debug, Clone)]
pub struct OrchestratorStatus {
    pub teams: usize,
    pub active_agents: usize,
    pub completed_tasks: usize,
}
