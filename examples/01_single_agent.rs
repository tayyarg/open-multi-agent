//! Example 01 -- Single Agent
//!
//! The simplest possible usage: one agent with bash and file tools, running
//! a coding task. Then shows multi-turn conversation using the Agent class.
//!
//! Run:
//!   cargo run --example single_agent
//!
//! Prerequisites:
//!   ANTHROPIC_API_KEY env var must be set.

use open_multi_agent::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // -------------------------------------------------------------------------
    // Part 1: Single agent via OpenMultiAgent (simplest path)
    // -------------------------------------------------------------------------

    let on_progress: Arc<dyn Fn(OrchestratorEvent) + Send + Sync> =
        Arc::new(|event: OrchestratorEvent| {
            match event.event_type {
                OrchestratorEventType::AgentStart => {
                    println!("[start]    agent={}", event.agent.as_deref().unwrap_or("?"));
                }
                OrchestratorEventType::AgentComplete => {
                    println!("[complete] agent={}", event.agent.as_deref().unwrap_or("?"));
                }
                _ => {}
            }
        });

    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".to_string()),
        on_progress: Some(Box::new(move |e| on_progress(e))),
        ..Default::default()
    });

    println!("Part 1: run_agent() -- single one-shot task\n");

    let result = orchestrator
        .run_agent(
            AgentConfig {
                name: "coder".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                system_prompt: Some(
                    "You are a focused Rust developer.\n\
                     When asked to implement something, write clean, minimal code with no extra commentary.\n\
                     Use the bash tool to run commands and the file tools to read/write files."
                        .to_string(),
                ),
                tools: vec![
                    "bash".to_string(),
                    "file_read".to_string(),
                    "file_write".to_string(),
                ],
                max_turns: Some(8),
                ..Default::default()
            },
            "Create a small Rust file at /tmp/greet.rs that:\n\
             1. Defines a function greet(name: &str) -> String\n\
             2. Returns \"Hello, <name>!\"\n\
             3. Has a main() that calls greet(\"World\") and prints the result.\n\
             Then compile and run it with: rustc /tmp/greet.rs -o /tmp/greet && /tmp/greet",
        )
        .await;

    if result.success {
        println!("\nAgent output:");
        println!("{}", "-".repeat(60));
        println!("{}", result.output);
        println!("{}", "-".repeat(60));
    } else {
        eprintln!("Agent failed: {}", result.output);
        std::process::exit(1);
    }

    println!("\nToken usage:");
    println!("  input:  {}", result.token_usage.input_tokens);
    println!("  output: {}", result.token_usage.output_tokens);
    println!("  tool calls made: {}", result.tool_calls.len());

    // -------------------------------------------------------------------------
    // Part 2: Multi-turn conversation via Agent.prompt()
    // -------------------------------------------------------------------------

    println!("\n\nPart 2: Agent.prompt() -- multi-turn conversation\n");

    let conv_registry = Arc::new(ToolRegistry::new());
    let conv_executor = Arc::new(ToolExecutor::new(conv_registry.clone(), None));

    let conversation_agent = Agent::new(
        AgentConfig {
            name: "tutor".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            system_prompt: Some("You are a Rust tutor. Give short, direct answers.".to_string()),
            max_turns: Some(2),
            ..Default::default()
        },
        conv_registry,
        conv_executor,
    );

    let turn1 = conversation_agent
        .prompt("What is a trait in Rust?")
        .await;
    let output1 = &turn1.output[..turn1.output.len().min(200)];
    println!("Turn 1: {}", output1);

    let turn2 = conversation_agent
        .prompt("Give me one concrete code example of what you just described.")
        .await;
    let output2 = &turn2.output[..turn2.output.len().min(300)];
    println!("\nTurn 2: {}", output2);

    let history = conversation_agent.get_history().await;
    println!("\nConversation history length: {} messages", history.len());

    println!("\nDone.");
}
