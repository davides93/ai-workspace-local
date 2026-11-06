//! The outcome of running a suite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraderResult {
    pub kind: String,
    pub passed: bool,
    /// 0..1: 1 or 0 for the deterministic graders, the judge's own score for `judge`.
    pub score: f64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    /// 1-based; more than one with `repeat`.
    pub attempt: u32,
    pub passed: bool,
    /// Mean score of the graders (0 when the run itself failed).
    pub score: f64,
    pub graders: Vec<GraderResult>,
    pub duration_ms: u64,
    pub tokens: u64,
    pub turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Why the run did not complete (failed, timed out…); the graders are still applied to what it left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The working folder, when it was kept (`--keep`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Attempts that passed.
    pub passed: u32,
    /// Attempts made.
    pub total: u32,
    /// Mean score of the attempts.
    pub score: f64,
    /// `passed / total`.
    pub pass_rate: f64,
}

/// `running` while tasks are still being run, then `completed`, or `failed` when the suite could not go on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub id: String,
    pub suite_id: String,
    pub suite_name: String,
    pub status: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// Agent and profile asked for on the command line or in the request (the suite's own otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub repeat: u32,
    pub tasks: Vec<TaskResult>,
    pub summary: Summary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl EvalReport {
    /// Recomputes `summary` from `tasks`.
    pub fn refresh_summary(&mut self) {
        let total = self.tasks.len() as u32;
        let passed = self.tasks.iter().filter(|t| t.passed).count() as u32;
        let mean = |f: &dyn Fn(&TaskResult) -> f64| if total == 0 { 0.0 } else { self.tasks.iter().map(f).sum::<f64>() / total as f64 };
        self.summary =
            Summary { passed, total, score: mean(&|t| t.score), pass_rate: if total == 0 { 0.0 } else { passed as f64 / total as f64 } };
    }
}
