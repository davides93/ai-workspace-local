//! Rewards for reinforcement learning: the graders of an evaluation task applied to an answer that
//! someone else produced (a trainer sampling from the model it is tuning). Only graders that look
//! at the answer itself can be used: the ones that need a working folder or a command are refused.

use crate::Engine;
use aiw_core::{Error, Result};
use aiw_eval::grade::{grade, GradeContext};
use aiw_eval::host::TaskRun;
use aiw_eval::{GraderResult, GraderSpec, Suite};
use serde::Serialize;

/// What `POST /api/evals/{id}/reward` answers.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RewardResult {
    /// Mean of the scores of the graders, 0..1.
    pub score: f64,
    /// Every grader passed.
    pub passed: bool,
    pub graders: Vec<GraderResult>,
}

/// Does this grader look only at the answer?
fn answer_only(g: &GraderSpec) -> bool {
    matches!(g, GraderSpec::OutputMatches { .. } | GraderSpec::JsonSchema { .. } | GraderSpec::Judge { .. })
}

/// Refuses a suite with a task whose graders need files or commands, naming the task and the grader.
pub(crate) fn check_reward_suite(suite: &Suite) -> Result<()> {
    for t in &suite.tasks {
        if let Some(g) = t.graders.iter().find(|g| !answer_only(g)) {
            return Err(Error::Invalid(format!(
                "il task «{}» della suite «{}» ha un grader `{}`: per la reward sono ammessi solo output_matches, json_schema e judge, \
                 che guardano la risposta e non file o comandi",
                t.id,
                suite.id,
                g.kind()
            )));
        }
    }
    Ok(())
}

impl Engine {
    /// The suite as a source of rewards, checked.
    pub fn reward_suite(&self, suite_id: &str) -> Result<Suite> {
        let suite = self.eval_suite(suite_id)?;
        check_reward_suite(&suite)?;
        Ok(suite)
    }

    /// Scores `output` as an answer to the task `task_id` of the suite.
    pub async fn reward(&self, suite_id: &str, task_id: &str, output: &str) -> Result<RewardResult> {
        let suite = self.eval_suite(suite_id)?;
        let task = suite.task(task_id).ok_or_else(|| Error::NotFound(format!("il task «{task_id}» non è nella suite «{suite_id}»")))?;
        if let Some(g) = task.graders.iter().find(|g| !answer_only(g)) {
            return Err(Error::Invalid(format!(
                "il task «{task_id}» ha un grader `{}`: per la reward sono ammessi solo output_matches, json_schema e judge",
                g.kind()
            )));
        }
        let run = TaskRun { final_text: output.to_string(), ..Default::default() };
        // The graders allowed here never touch the working folder.
        let workdir = std::env::temp_dir();
        let ctx = GradeContext { workdir: &workdir, run: &run, host: self, prompt: &task.prompt, changes: &[] };
        let mut graders = Vec::with_capacity(task.graders.len());
        for g in &task.graders {
            graders.push(grade(g, &ctx).await);
        }
        let score = if graders.is_empty() { 0.0 } else { graders.iter().map(|g| g.score).sum::<f64>() / graders.len() as f64 };
        Ok(RewardResult { score, passed: !graders.is_empty() && graders.iter().all(|g| g.passed), graders })
    }
}
