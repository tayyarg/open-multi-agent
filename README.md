# Open Multi-Agent (Rust)

Build AI agent teams that work together. One agent plans, another implements, a third reviews -- the framework handles task scheduling, dependencies, and communication automatically.

[![GitHub stars](https://img.shields.io/github/stars/tayyarg/open-multi-agent)](https://github.com/tayyarg/open-multi-agent/stargazers)
[![license](https://img.shields.io/github/license/tayyarg/open-multi-agent)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange)](https://www.rust-lang.org/)

**English** | [中文](./README_zh.md)

## Why Open Multi-Agent?

- **Multi-Agent Teams** -- Define agents with different roles, tools, and even different models. They collaborate through a message bus and shared memory.
- **Task DAG Scheduling** -- Tasks have dependencies. The framework resolves them topologically -- dependent tasks wait, independent tasks run in parallel.
- **Model Agnostic** -- Claude, GPT, and local models (Ollama, vLLM, LM Studio) in the same team. Swap models per agent via `base_url`.
- **In-Process Execution** -- No subprocess overhead. Everything runs in one tokio async runtime. Deploy to serverless, Docker, CI/CD.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
open-multi-agent = { git = "https://github.com/tayyarg/open-multi-agent.git" }
tokio = { version = "1", features = ["full"] }
```

Set `ANTHROPIC_API_KEY` (and optionally `OPENAI_API_KEY` or `GITHUB_TOKEN` for Copilot) in your environment.

```rust
use open_multi_agent::prelude::*;

#[tokio::main]
async fn main() {
    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".into()),
        ..Default::default()
    });

    // One agent, one task
    let result = orchestrator.run_agent(
        AgentConfig {
            name: "coder".into(),
            model: "claude-sonnet-4-6".into(),
            tools: vec!["bash".into(), "file_write".into()],
            ..Default::default()
        },
        "Write a Rust function that reverses a string, save it to /tmp/reverse.rs, and run it.",
    ).await;

    println!("{}", result.output);
}
```

## Multi-Agent Team

This is where it gets interesting. Three agents, one goal:

```rust
use open_multi_agent::prelude::*;

#[tokio::main]
async fn main() {
    let architect = AgentConfig {
        name: "architect".into(),
        model: "claude-sonnet-4-6".into(),
        system_prompt: Some("You design clean API contracts and file structures.".into()),
        tools: vec!["file_write".into()],
        ..Default::default()
    };

    let developer = AgentConfig {
        name: "developer".into(),
        model: "claude-sonnet-4-6".into(),
        system_prompt: Some("You implement what the architect designs.".into()),
        tools: vec!["bash".into(), "file_read".into(), "file_write".into(), "file_edit".into()],
        ..Default::default()
    };

    let reviewer = AgentConfig {
        name: "reviewer".into(),
        model: "claude-sonnet-4-6".into(),
        system_prompt: Some("You review code for correctness and clarity.".into()),
        tools: vec!["file_read".into(), "grep".into()],
        ..Default::default()
    };

    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".into()),
        ..Default::default()
    });

    orchestrator.create_team("api-team", TeamConfig {
        name: "api-team".into(),
        agents: vec![architect, developer, reviewer],
        shared_memory: true,
        max_concurrency: None,
    });

    // Describe a goal -- the framework breaks it into tasks and orchestrates execution
    let result = orchestrator
        .run_team("api-team", "Create a REST API for a todo list in /tmp/todo-api/")
        .await
        .unwrap();

    println!("Success: {}", result.success);
    println!("Tokens: {} output tokens", result.total_token_usage.output_tokens);
}
```

## More Examples

<details>
<summary><b>Task Pipeline</b> -- explicit control over task graph and assignments</summary>

```rust
let tasks = vec![
    ParsedTaskSpecInput {
        title: "Design the data model".into(),
        description: "Write a spec to /tmp/spec.md".into(),
        assignee: Some("architect".into()),
        depends_on: None,
    },
    ParsedTaskSpecInput {
        title: "Implement the module".into(),
        description: "Read /tmp/spec.md and implement the module in /tmp/src/".into(),
        assignee: Some("developer".into()),
        depends_on: Some(vec!["Design the data model".into()]), // blocked until design completes
    },
    ParsedTaskSpecInput {
        title: "Write tests".into(),
        description: "Read the implementation and write tests.".into(),
        assignee: Some("developer".into()),
        depends_on: Some(vec!["Implement the module".into()]),
    },
    ParsedTaskSpecInput {
        title: "Review code".into(),
        description: "Review /tmp/src/ and produce a structured code review.".into(),
        assignee: Some("reviewer".into()),
        depends_on: Some(vec!["Implement the module".into()]), // can run in parallel with tests
    },
];

let result = orchestrator.run_tasks("pipeline-team", tasks).await.unwrap();
```

</details>

<details>
<summary><b>Custom Tools</b> -- implement the ToolDefinition trait</summary>

```rust
use async_trait::async_trait;
use open_multi_agent::prelude::*;
use std::sync::Arc;

struct WebSearchTool;

#[async_trait]
impl ToolDefinition for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str { "Search the web and return the top results." }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search query." }
            },
            "required": ["query"]
        })
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolUseContext) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("");
        // your search logic here
        ToolResult { data: format!("Results for: {}", query), is_error: false }
    }
}

let mut registry = ToolRegistry::new();
register_built_in_tools(&mut registry);
registry.register(Arc::new(WebSearchTool));
let registry = Arc::new(registry);
let executor = Arc::new(ToolExecutor::new(registry.clone(), None));

let agent = Agent::new(
    AgentConfig {
        name: "researcher".into(),
        model: "claude-sonnet-4-6".into(),
        tools: vec!["web_search".into()],
        ..Default::default()
    },
    registry,
    executor,
);

let result = agent.run("Find the three most recent Rust releases.").await;
```

</details>

<details>
<summary><b>Multi-Model Teams</b> -- mix Claude, GPT, and local models in one workflow</summary>

```rust
let claude_agent = AgentConfig {
    name: "strategist".into(),
    model: "claude-opus-4-6".into(),
    provider: Some(Provider::Anthropic),
    system_prompt: Some("You plan high-level approaches.".into()),
    tools: vec!["file_write".into()],
    ..Default::default()
};

let gpt_agent = AgentConfig {
    name: "implementer".into(),
    model: "gpt-4o".into(),
    provider: Some(Provider::OpenAI),
    system_prompt: Some("You implement plans as working code.".into()),
    tools: vec!["bash".into(), "file_read".into(), "file_write".into()],
    ..Default::default()
};

// Any OpenAI-compatible API -- Ollama, vLLM, LM Studio, etc.
let local_agent = AgentConfig {
    name: "reviewer".into(),
    model: "llama3.1".into(),
    provider: Some(Provider::OpenAI),
    base_url: Some("http://localhost:11434/v1".into()),
    api_key: Some("ollama".into()),
    system_prompt: Some("You review code for correctness and clarity.".into()),
    tools: vec!["file_read".into(), "grep".into()],
    ..Default::default()
};

orchestrator.create_team("mixed-team", TeamConfig {
    name: "mixed-team".into(),
    agents: vec![claude_agent, gpt_agent, local_agent],
    shared_memory: true,
    max_concurrency: None,
});

let result = orchestrator
    .run_team("mixed-team", "Build a CLI tool that converts JSON to CSV.")
    .await
    .unwrap();
```

</details>

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  OpenMultiAgent (Orchestrator)                                  │
│                                                                 │
│  create_team()  run_team()  run_tasks()  run_agent()            │
└──────────────────────┬──────────────────────────────────────────┘
                       │
            ┌──────────▼──────────┐
            │  Team               │
            │  - AgentConfig[]    │
            │  - MessageBus       │
            │  - TaskQueue        │
            │  - SharedMemory     │
            └──────────┬──────────┘
                       │
         ┌─────────────┴─────────────┐
         │                           │
┌────────▼──────────┐    ┌───────────▼───────────┐
│  AgentPool        │    │  TaskQueue             │
│  - Semaphore      │    │  - dependency graph    │
│  - run_parallel() │    │  - auto unblock        │
└────────┬──────────┘    │  - cascade failure     │
         │               └───────────────────────┘
┌────────▼──────────┐
│  Agent            │
│  - run()          │    ┌──────────────────────┐
│  - prompt()       │───►│  LlmAdapter (trait)  │
└────────┬──────────┘    │  - AnthropicAdapter  │
         │               │  - OpenAIAdapter     │
         │               │  - CopilotAdapter    │
         │               └──────────────────────┘
┌────────▼──────────┐
│  AgentRunner      │    ┌──────────────────────┐
│  - conversation   │───►│  ToolRegistry        │
│    loop           │    │  - ToolDefinition    │
│  - tool dispatch  │    │  - 5 built-in tools  │
└───────────────────┘    └──────────────────────┘
```

## Built-in Tools

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands. Returns stdout + stderr. Supports timeout and cwd. |
| `file_read` | Read file contents at an absolute path. Supports offset/limit for large files. |
| `file_write` | Write or create a file. Auto-creates parent directories. |
| `file_edit` | Edit a file by replacing an exact string match. |
| `grep` | Search file contents with regex. Uses ripgrep when available, falls back to pure Rust regex. |

## Running Examples

```bash
# Single agent
cargo run --example single_agent

# Team collaboration
cargo run --example team_collaboration

# Task pipeline with dependencies
cargo run --example task_pipeline

# Multi-model team with custom tools
cargo run --example multi_model_team

# Copilot adapter smoke test
cargo run --example copilot_test
```

## Contributing

Issues, feature requests, and PRs are welcome. Some areas where contributions would be especially valuable:

- **LLM Adapters** -- Anthropic, OpenAI, and Copilot are supported out of the box. Any OpenAI-compatible API (Ollama, vLLM, LM Studio, etc.) works via `base_url`. Additional adapters for Gemini and other providers are welcome. The `LlmAdapter` trait requires just two methods: `chat()` and `stream()`.
- **Examples** -- Real-world workflows and use cases.
- **Documentation** -- Guides, tutorials, and API docs.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=tayyarg/open-multi-agent&type=Date)](https://star-history.com/#tayyarg/open-multi-agent&Date)

## Contributors

<a href="https://github.com/tayyarg/open-multi-agent/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=tayyarg/open-multi-agent" />
</a>

## License

MIT
