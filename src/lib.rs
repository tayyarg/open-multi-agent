//! open-multi-agent -- Production-grade multi-agent orchestration framework.
//!
//! Import from `open_multi_agent` to access everything you need:
//!
//! ```rust,ignore
//! use open_multi_agent::{OpenMultiAgent, Agent, Team};
//! ```
//!
//! ## Quickstart
//!
//! ### Single agent
//! ```rust,ignore
//! use open_multi_agent::prelude::*;
//!
//! let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig::default());
//! let result = orchestrator.run_agent(
//!     AgentConfig {
//!         name: "assistant".into(),
//!         model: "claude-opus-4-6".into(),
//!         ..Default::default()
//!     },
//!     "Explain monads in one paragraph.",
//! ).await;
//! println!("{}", result.output);
//! ```
//!
//! ### Multi-agent team (auto-orchestrated)
//! ```rust,ignore
//! use open_multi_agent::prelude::*;
//!
//! let mut orchestrator = OpenMultiAgent::new(OrchestratorConfig {
//!     default_model: Some("claude-opus-4-6".into()),
//!     ..Default::default()
//! });
//!
//! orchestrator.create_team("writers", TeamConfig {
//!     name: "writers".into(),
//!     agents: vec![
//!         AgentConfig { name: "researcher".into(), model: "claude-opus-4-6".into(), ..Default::default() },
//!         AgentConfig { name: "writer".into(), model: "claude-opus-4-6".into(), ..Default::default() },
//!     ],
//!     shared_memory: true,
//!     max_concurrency: None,
//! });
//!
//! let result = orchestrator.run_team("writers", "Write a guide on Rust generics.").await.unwrap();
//! ```

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

pub mod agent;
pub mod llm;
pub mod memory;
pub mod orchestrator;
pub mod task;
pub mod team;
pub mod tool;
pub mod types;
pub mod utils;

// ---------------------------------------------------------------------------
// Orchestrator (primary entry point)
// ---------------------------------------------------------------------------

pub use orchestrator::orchestrator::{OpenMultiAgent, OrchestratorStatus, ParsedTaskSpecInput};
pub use orchestrator::scheduler::{Scheduler, SchedulingStrategy};

// ---------------------------------------------------------------------------
// Agent layer
// ---------------------------------------------------------------------------

pub use agent::agent::Agent;
pub use agent::pool::{AgentPool, PoolStatus};

// ---------------------------------------------------------------------------
// Team layer
// ---------------------------------------------------------------------------

pub use team::team::Team;
pub use team::messaging::{Message, MessageBus};

// ---------------------------------------------------------------------------
// Task layer
// ---------------------------------------------------------------------------

pub use task::queue::{TaskQueue, TaskQueueEvent, TaskProgress};
pub use task::task::{create_task, is_task_ready, get_task_dependency_order, validate_task_dependencies};

// ---------------------------------------------------------------------------
// Tool system
// ---------------------------------------------------------------------------

pub use tool::framework::{ToolDefinition, ToolRegistry};
pub use tool::executor::ToolExecutor;
pub use tool::built_in::register_built_in_tools;

// ---------------------------------------------------------------------------
// LLM adapters
// ---------------------------------------------------------------------------

pub use llm::adapter::{LlmAdapter, create_adapter};

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub use memory::store::{MemoryStore, InMemoryStore};
pub use memory::shared::SharedMemory;

// ---------------------------------------------------------------------------
// Types -- all public types re-exported
// ---------------------------------------------------------------------------

pub use types::*;

// ---------------------------------------------------------------------------
// Prelude -- convenient glob import
// ---------------------------------------------------------------------------

pub mod prelude {
    pub use crate::{
        OpenMultiAgent, OrchestratorStatus, ParsedTaskSpecInput,
        Scheduler, SchedulingStrategy,
        Agent, AgentPool, PoolStatus,
        Team, Message, MessageBus,
        TaskQueue, TaskQueueEvent, TaskProgress,
        create_task, is_task_ready, get_task_dependency_order, validate_task_dependencies,
        ToolDefinition, ToolRegistry, ToolExecutor,
        register_built_in_tools,
        create_adapter,
        InMemoryStore, SharedMemory,
    };
    pub use crate::types::*;
}
