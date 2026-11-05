//! Evaluation of agents: a suite of tasks, the graders that decide whether each one passed, and the
//! report of a run. The crate knows nothing about how an agent runs: the engine implements
//! [`EvalHost`] (architecture §5.4).

pub mod grade;
pub mod host;
pub mod report;
pub mod runner;
pub mod suite;

pub use host::{EvalHost, TaskRequest, TaskRun};
pub use report::{EvalReport, GraderResult, Summary, TaskResult};
pub use runner::{run_suite, EvalEvent, RunOptions, DEFAULT_AGENT, MAX_REPEAT};
pub use suite::{GraderSpec, Suite, Task};
