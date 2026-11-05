//! What the evaluation needs from the engine.

use aiw_core::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// One task handed to an agent.
#[derive(Debug, Clone)]
pub struct TaskRequest {
    pub agent: String,
    /// Inference profile that replaces the agent's own.
    pub profile: Option<String>,
    pub prompt: String,
    /// The folder the agent works in (its workspace root), already holding the task's files.
    pub workdir: PathBuf,
    /// Wall-clock limit of the run.
    pub timeout: Duration,
}

/// What a task run left behind, besides the files in the working folder.
#[derive(Debug, Clone, Default)]
pub struct TaskRun {
    pub final_text: String,
    /// The final answer as JSON, when the agent has an output schema.
    pub structured: Option<Value>,
    pub duration_ms: u64,
    pub tokens: u64,
    pub turns: u32,
    pub run_id: Option<String>,
    /// The run failed, was stopped or ran out of time.
    pub error: Option<String>,
}

#[async_trait]
pub trait EvalHost: Send + Sync {
    /// Runs the task to the end. `Err` is for a host that could not even start it (unknown agent…);
    /// a run that fails comes back as `Ok` with `error` set.
    async fn run_task(&self, req: TaskRequest) -> Result<TaskRun>;

    /// One answer of the model of inference profile `profile` (the default profile when `None`) to a
    /// prompt, with no tools: what a `judge` grader asks.
    async fn complete(&self, profile: Option<&str>, prompt: &str) -> Result<String>;
}
