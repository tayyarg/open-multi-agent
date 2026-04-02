//! Quick smoke test for the Copilot adapter.
//!
//! Run:
//!   cargo run --example copilot_test
//!
//! If GITHUB_COPILOT_TOKEN is not set, the adapter will start an interactive
//! OAuth2 device flow -- you'll be prompted to sign in via your browser.

use open_multi_agent::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() {
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
        default_model: Some("gpt-4o".to_string()),
        default_provider: Some(Provider::Copilot),
        on_progress: Some(Box::new(move |e| on_progress(e))),
        ..Default::default()
    });

    println!("Testing Copilot adapter with gpt-4o...\n");

    let result = orchestrator
        .run_agent(
            AgentConfig {
                name: "assistant".to_string(),
                model: "gpt-4o".to_string(),
                provider: Some(Provider::Copilot),
                system_prompt: Some(
                    "You are a helpful assistant. Keep answers brief.".to_string(),
                ),
                max_turns: Some(1),
                max_tokens: Some(256),
                ..Default::default()
            },
            "What is 2 + 2? Reply in one sentence.",
        )
        .await;

    if result.success {
        println!("\nAgent output:");
        println!("{}", "-".repeat(60));
        println!("{}", result.output);
        println!("{}", "-".repeat(60));
        println!(
            "\nTokens: input={}, output={}",
            result.token_usage.input_tokens, result.token_usage.output_tokens
        );
    } else {
        eprintln!("Agent failed: {}", result.output);
        std::process::exit(1);
    }
}
