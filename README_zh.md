# Open Multi-Agent

构建能协同工作的 AI 智能体团队。一个智能体负责规划，一个负责实现，一个负责审查——框架自动处理任务调度、依赖关系和智能体间通信。

[![GitHub stars](https://img.shields.io/github/stars/tayyarg/open-multi-agent)](https://github.com/tayyarg/open-multi-agent/stargazers)
[![license](https://img.shields.io/github/license/tayyarg/open-multi-agent)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange)](https://www.rust-lang.org/)

[English](./README.md) | **中文**

## 为什么选择 Open Multi-Agent？

- **多智能体团队** — 定义不同角色、工具甚至不同模型的智能体。它们通过消息总线和共享内存协作。
- **任务 DAG 调度** — 任务之间存在依赖关系。框架进行拓扑排序——有依赖的任务等待，无依赖的任务并行执行。
- **模型无关** — Claude、GPT 和本地模型（Ollama、vLLM、LM Studio）可以在同一个团队中使用。通过 `base_url` 即可接入任何 OpenAI 兼容服务。
- **进程内执行** — 没有子进程开销。所有内容在一个 tokio 异步运行时中运行。可部署到 Serverless、Docker、CI/CD。

## 快速开始

在 `Cargo.toml` 中添加：

```toml
[dependencies]
open-multi-agent = { git = "https://github.com/tayyarg/open-multi-agent.git" }
tokio = { version = "1", features = ["full"] }
```

在环境变量中设置 `ANTHROPIC_API_KEY`（以及可选的 `OPENAI_API_KEY` 或用于 Copilot 的 `GITHUB_TOKEN`）。

```rust
use open_multi_agent::prelude::*;

#[tokio::main]
async fn main() {
    let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
        default_model: Some("claude-sonnet-4-6".into()),
        ..Default::default()
    });

    // 一个智能体，一个任务
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

## 多智能体团队

这才是有意思的地方。三个智能体，一个目标：

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

    // 描述一个目标——框架将其拆解为任务并编排执行
    let result = orchestrator
        .run_team("api-team", "Create a REST API for a todo list in /tmp/todo-api/")
        .await
        .unwrap();

    println!("成功: {}", result.success);
    println!("Token 用量: {} output tokens", result.total_token_usage.output_tokens);
}
```

## 更多示例

<details>
<summary><b>任务流水线</b> — 显式控制任务图和分配</summary>

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
        depends_on: Some(vec!["Design the data model".into()]), // 等待设计完成后才开始
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
        depends_on: Some(vec!["Implement the module".into()]), // 可以和测试并行执行
    },
];

let result = orchestrator.run_tasks("pipeline-team", tasks).await.unwrap();
```

</details>

<details>
<summary><b>自定义工具</b> — 实现 ToolDefinition trait</summary>

```rust
use async_trait::async_trait;
use open_multi_agent::prelude::*;
use std::sync::Arc;

struct WebSearchTool;

#[async_trait]
impl ToolDefinition for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str { "搜索网页并返回结果。" }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索查询。" }
            },
            "required": ["query"]
        })
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolUseContext) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("");
        // 你的搜索逻辑
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

let result = agent.run("查找最近三个 Rust 版本。").await;
```

</details>

<details>
<summary><b>多模型团队</b> — 在一个工作流中混合使用 Claude、GPT 和本地模型</summary>

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

// 任何 OpenAI 兼容 API — Ollama、vLLM、LM Studio 等
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

## 架构

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

## 内置工具

| 工具 | 说明 |
|------|------|
| `bash` | 执行 Shell 命令。返回 stdout + stderr。支持超时和工作目录设置。 |
| `file_read` | 读取指定绝对路径的文件内容。支持偏移量和行数限制以处理大文件。 |
| `file_write` | 写入或创建文件。自动创建父目录。 |
| `file_edit` | 通过精确字符串匹配编辑文件。 |
| `grep` | 使用正则表达式搜索文件内容。优先使用 ripgrep，回退到纯 Rust regex。 |

## 运行示例

```bash
# 单智能体
cargo run --example single_agent

# 团队协作
cargo run --example team_collaboration

# 带依赖的任务流水线
cargo run --example task_pipeline

# 多模型团队与自定义工具
cargo run --example multi_model_team

# Copilot 适配器测试
cargo run --example copilot_test
```

## 参与贡献

欢迎提 Issue、功能需求和 PR。以下方向的贡献尤其有价值：

- **LLM 适配器** — Anthropic、OpenAI、Copilot 已原生支持。任何 OpenAI 兼容 API（Ollama、vLLM、LM Studio 等）可通过 `base_url` 直接使用。欢迎贡献 Gemini 等其他适配器。`LlmAdapter` trait 只需实现两个方法：`chat()` 和 `stream()`。
- **示例** — 真实场景的工作流和用例。
- **文档** — 指南、教程和 API 文档。

## Star 趋势

[![Star History Chart](https://api.star-history.com/svg?repos=tayyarg/open-multi-agent&type=Date)](https://star-history.com/#tayyarg/open-multi-agent&Date)

## 贡献者

<a href="https://github.com/tayyarg/open-multi-agent/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=tayyarg/open-multi-agent" />
</a>

## 许可证

MIT
