//! Example 02 -- Multi-Agent Team Collaboration
//!
//! Three specialised agents (architect, developer, reviewer) collaborate on a
//! shared goal. The OpenMultiAgent orchestrator breaks the goal into tasks, assigns
//! them to the right agents, and collects the results.
//!
//! Run:
//!   cargo run --example team_collaboration
//!
//! Prerequisites:
//!   ANTHROPIC_API_KEY env var must be set.

use open_multi_agent::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[tokio::main]
async fn main() {
    // -------------------------------------------------------------------------
    // Agent definitions
    // -------------------------------------------------------------------------

    let architect = AgentConfig {
        name: "architect".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        provider: Some(Provider::Anthropic),
        system_prompt: Some(
            "You are a software architect with deep experience in Node.js and REST API design.\n\
             Your job is to design clear, production-quality API contracts and file/directory structures.\n\
             Output concise plans in markdown -- no unnecessary prose."
                .to_string(),
        ),
        tools: vec!["bash".to_string(), "file_write".to_string()],
        max_turns: Some(5),
        temperature: Some(0.2),
        ..Default::default()
    };

    let developer = AgentConfig {
        name: "developer".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        provider: Some(Provider::Anthropic),
        system_prompt: Some(
            "You are a TypeScript/Node.js developer. You implement what the architect specifies.\n\
             Write clean, runnable code with proper error handling. Use the tools to write files and run tests."
                .to_string(),
        ),
        tools: vec![
            "bash".to_string(),
            "file_read".to_string(),
            "file_write".to_string(),
            "file_edit".to_string(),
        ],
        max_turns: Some(12),
        temperature: Some(0.1),
        ..Default::default()
    };

    let reviewer = AgentConfig {
        name: "reviewer".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        provider: Some(Provider::Anthropic),
        system_prompt: Some(
            "You are a senior code reviewer. Review code for correctness, security, and clarity.\n\
             Provide a structured review with: LGTM items, suggestions, and any blocking issues.\n\
             Read files using the tools before reviewing."
                .to_string(),
        ),
        tools: vec![
            "bash".to_string(),
            "file_read".to_string(),
            "grep".to_string(),
        ],
        max_turns: Some(5),
        temperature: Some(0.3),
        ..Default::default()
    };

    // -------------------------------------------------------------------------
    // Progress tracking
    // -------------------------------------------------------------------------

    let start_times: Arc<Mutex<HashMap<String, Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let start_times_clone = start_times.clone();

    let on_progress: Box<dyn Fn(OrchestratorEvent) + Send + Sync> =
        Box::new(move |event: OrchestratorEvent| {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();

            match event.event_type {
                OrchestratorEventType::AgentStart => {
                    let agent = event.agent.as_deref().unwrap_or("?");
                    start_times_clone
                        .lock()
                        .unwrap()
                        .insert(agent.to_string(), Instant::now());
                    println!("[{}] AGENT START  -> {}", now, agent);
                }
                OrchestratorEventType::AgentComplete => {
                    let agent = event.agent.as_deref().unwrap_or("?");
                    let elapsed = start_times_clone
                        .lock()
                        .unwrap()
                        .get(agent)
                        .map(|s| s.elapsed().as_millis())
                        .unwrap_or(0);
                    println!("[{}] AGENT DONE   <- {} ({}ms)", now, agent, elapsed);
                }
                OrchestratorEventType::TaskStart => {
                    println!("[{}] TASK START   | {}", now, event.task.as_deref().unwrap_or("?"));
                }
                OrchestratorEventType::TaskComplete => {
                    println!("[{}] TASK DONE    ^ {}", now, event.task.as_deref().unwrap_or("?"));
                }
                OrchestratorEventType::Error => {
                    eprintln!(
                        "[{}] ERROR        x agent={} task={}",
                        now,
                        event.agent.as_deref().unwrap_or("?"),
                        event.task.as_deref().unwrap_or("?")
                    );
                }
                _ => {}
            }
        });

    // -------------------------------------------------------------------------
    // Orchestrate
    // -------------------------------------------------------------------------

    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".to_string()),
        max_concurrency: Some(1),
        on_progress: Some(on_progress),
        ..Default::default()
    });

    let team_config = TeamConfig {
        name: "api-team".to_string(),
        agents: vec![architect, developer, reviewer],
        shared_memory: true,
        max_concurrency: Some(1),
    };

    orchestrator.create_team("api-team", team_config);
    println!("Team \"api-team\" created.\n");
    println!("Starting team run...\n");
    println!("{}", "=".repeat(60));

    let goal = "Create a minimal Express.js REST API in /tmp/express-api/ with:\n\
                - GET  /health       -> { status: \"ok\" }\n\
                - GET  /users        -> returns a hardcoded array of 2 user objects\n\
                - POST /users        -> accepts { name, email } body, logs it, returns 201\n\
                - Proper error handling middleware\n\
                - The server should listen on port 3001\n\
                - Include a package.json with the required dependencies";

    let result = orchestrator.run_team("api-team", goal).await;

    println!("\n{}", "=".repeat(60));

    match result {
        Ok(result) => {
            println!("\nTeam run complete.");
            println!("Success: {}", result.success);
            println!(
                "Total tokens -- input: {}, output: {}",
                result.total_token_usage.input_tokens, result.total_token_usage.output_tokens
            );

            println!("\nPer-agent results:");
            for (agent_name, agent_result) in &result.agent_results {
                let status = if agent_result.success { "OK" } else { "FAILED" };
                let tools = agent_result.tool_calls.len();
                println!("  {:12} [{}]  tool_calls={}", agent_name, status, tools);
                if !agent_result.success {
                    let snippet = &agent_result.output[..agent_result.output.len().min(120)];
                    println!("    Error: {}", snippet);
                }
            }
        }
        Err(e) => {
            eprintln!("Team run failed: {}", e);
            std::process::exit(1);
        }
    }
}
