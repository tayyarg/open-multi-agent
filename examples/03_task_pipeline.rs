//! Example 03 -- Explicit Task Pipeline with Dependencies
//!
//! Demonstrates how to define tasks with explicit dependency chains
//! (design -> implement -> test -> review) using run_tasks(). The TaskQueue
//! automatically blocks downstream tasks until their dependencies complete.
//!
//! Run:
//!   cargo run --example task_pipeline
//!
//! Prerequisites:
//!   ANTHROPIC_API_KEY env var must be set.

use open_multi_agent::prelude::*;

#[tokio::main]
async fn main() {
    // -------------------------------------------------------------------------
    // Agents
    // -------------------------------------------------------------------------

    let designer = AgentConfig {
        name: "designer".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        system_prompt: Some(
            "You are a software designer. Your output is always a concise technical spec\n\
             in markdown. Focus on interfaces, data shapes, and file structure. Be brief."
                .to_string(),
        ),
        tools: vec!["file_write".to_string()],
        max_turns: Some(4),
        ..Default::default()
    };

    let implementer = AgentConfig {
        name: "implementer".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        system_prompt: Some(
            "You are a TypeScript developer. Read the design spec written by the designer,\n\
             then implement it. Write all files to /tmp/pipeline-output/. Use the tools."
                .to_string(),
        ),
        tools: vec![
            "bash".to_string(),
            "file_read".to_string(),
            "file_write".to_string(),
        ],
        max_turns: Some(10),
        ..Default::default()
    };

    let tester = AgentConfig {
        name: "tester".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        system_prompt: Some(
            "You are a QA engineer. Read the implemented files and run them to verify correctness.\n\
             Report: what passed, what failed, and any bugs found."
                .to_string(),
        ),
        tools: vec![
            "bash".to_string(),
            "file_read".to_string(),
            "grep".to_string(),
        ],
        max_turns: Some(6),
        ..Default::default()
    };

    let reviewer = AgentConfig {
        name: "reviewer".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        system_prompt: Some(
            "You are a code reviewer. Read all files and produce a brief structured review.\n\
             Sections: Summary, Strengths, Issues (if any), Verdict (SHIP / NEEDS WORK)."
                .to_string(),
        ),
        tools: vec!["file_read".to_string(), "grep".to_string()],
        max_turns: Some(4),
        ..Default::default()
    };

    // -------------------------------------------------------------------------
    // Progress handler
    // -------------------------------------------------------------------------

    let on_progress: Box<dyn Fn(OrchestratorEvent) + Send + Sync> =
        Box::new(|event: OrchestratorEvent| {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f").to_string();

            match event.event_type {
                OrchestratorEventType::TaskStart => {
                    println!(
                        "[{}] TASK READY    \"{}\"",
                        now,
                        event.task.as_deref().unwrap_or("?")
                    );
                }
                OrchestratorEventType::TaskComplete => {
                    println!(
                        "[{}] TASK DONE     \"{}\"",
                        now,
                        event.task.as_deref().unwrap_or("?")
                    );
                }
                OrchestratorEventType::AgentStart => {
                    println!(
                        "[{}] AGENT START   {}",
                        now,
                        event.agent.as_deref().unwrap_or("?")
                    );
                }
                OrchestratorEventType::AgentComplete => {
                    println!(
                        "[{}] AGENT DONE    {}",
                        now,
                        event.agent.as_deref().unwrap_or("?")
                    );
                }
                OrchestratorEventType::Error => {
                    eprintln!(
                        "[{}] ERROR         {}  task=\"{}\"",
                        now,
                        event.agent.as_deref().unwrap_or(""),
                        event.task.as_deref().unwrap_or("?")
                    );
                }
                _ => {}
            }
        });

    // -------------------------------------------------------------------------
    // Build the pipeline
    // -------------------------------------------------------------------------

    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".to_string()),
        max_concurrency: Some(2),
        on_progress: Some(on_progress),
        ..Default::default()
    });

    orchestrator.create_team(
        "pipeline-team",
        TeamConfig {
            name: "pipeline-team".to_string(),
            agents: vec![designer, implementer, tester, reviewer],
            shared_memory: true,
            max_concurrency: None,
        },
    );

    let spec_file = "/tmp/pipeline-output/design-spec.md";

    let tasks = vec![
        ParsedTaskSpecInput {
            title: "Design: URL shortener data model".to_string(),
            description: format!(
                "Design a minimal in-memory URL shortener service.\n\
                 Write a markdown spec to {} covering:\n\
                 - TypeScript interfaces for Url and ShortenRequest\n\
                 - The shortening algorithm (hash approach is fine)\n\
                 - API contract: POST /shorten, GET /:code\n\
                 Keep the spec under 30 lines.",
                spec_file
            ),
            assignee: Some("designer".to_string()),
            depends_on: None,
        },
        ParsedTaskSpecInput {
            title: "Implement: URL shortener".to_string(),
            description: format!(
                "Read the design spec at {}.\n\
                 Implement the URL shortener in /tmp/pipeline-output/src/:\n\
                 - shortener.ts: core logic (shorten, resolve functions)\n\
                 - server.ts: tiny HTTP server using Node's built-in http module (no Express)\n\
                   - POST /shorten  body: {{ url: string }} -> {{ code: string, short: string }}\n\
                   - GET  /:code    -> redirect (301) or 404\n\
                 - index.ts: entry point that starts the server on port 3002\n\
                 No external dependencies beyond Node built-ins.",
                spec_file
            ),
            assignee: Some("implementer".to_string()),
            depends_on: Some(vec!["Design: URL shortener data model".to_string()]),
        },
        ParsedTaskSpecInput {
            title: "Test: URL shortener".to_string(),
            description: "Run the URL shortener implementation:\n\
                          1. Start the server: node /tmp/pipeline-output/src/index.ts (or tsx)\n\
                          2. POST a URL to shorten it using curl\n\
                          3. Verify the GET redirect works\n\
                          4. Report what passed and what (if anything) failed.\n\
                          Kill the server after testing."
                .to_string(),
            assignee: Some("tester".to_string()),
            depends_on: Some(vec!["Implement: URL shortener".to_string()]),
        },
        ParsedTaskSpecInput {
            title: "Review: URL shortener".to_string(),
            description: "Read all .ts files in /tmp/pipeline-output/src/ and the design spec.\n\
                          Produce a structured code review with sections:\n\
                          - Summary (2 sentences)\n\
                          - Strengths (bullet list)\n\
                          - Issues (bullet list, or \"None\" if clean)\n\
                          - Verdict: SHIP or NEEDS WORK"
                .to_string(),
            assignee: Some("reviewer".to_string()),
            depends_on: Some(vec!["Implement: URL shortener".to_string()]),
        },
    ];

    // -------------------------------------------------------------------------
    // Run
    // -------------------------------------------------------------------------

    println!("Starting 4-stage task pipeline...\n");
    println!("Pipeline: design -> implement -> test + review (parallel)");
    println!("{}", "=".repeat(60));

    let result = orchestrator
        .run_tasks("pipeline-team", tasks)
        .await;

    // -------------------------------------------------------------------------
    // Summary
    // -------------------------------------------------------------------------

    println!("\n{}", "=".repeat(60));
    println!("Pipeline complete.\n");

    match result {
        Ok(result) => {
            println!("Overall success: {}", result.success);
            println!(
                "Tokens -- input: {}, output: {}",
                result.total_token_usage.input_tokens, result.total_token_usage.output_tokens
            );

            println!("\nPer-agent summary:");
            for (name, r) in &result.agent_results {
                let icon = if r.success { "OK  " } else { "FAIL" };
                let tool_names: Vec<&str> = r.tool_calls.iter().map(|c| c.tool_name.as_str()).collect();
                println!("  [{}] {:14}  tools used: {}", icon, name, tool_names.join(", "));
            }

            if let Some(review) = result.agent_results.get("reviewer") {
                if review.success {
                    println!("\nCode review:");
                    println!("{}", "-".repeat(60));
                    println!("{}", review.output);
                    println!("{}", "-".repeat(60));
                }
            }
        }
        Err(e) => {
            eprintln!("Pipeline failed: {}", e);
            std::process::exit(1);
        }
    }
}
