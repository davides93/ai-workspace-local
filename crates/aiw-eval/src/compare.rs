//! Side-by-side comparison of two reports (A/B): per task and in total.

use crate::report::EvalReport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which report a side of the comparison is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRef {
    pub id: String,
    pub suite_id: String,
    pub agent: Option<String>,
    pub profile: Option<String>,
}

/// What a report says about one task (all its attempts together).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Side {
    pub attempts: u32,
    pub passed: u32,
    /// `passed / attempts`.
    pub pass_rate: f64,
    pub score: f64,
    pub avg_tokens: f64,
    pub avg_duration_ms: f64,
}

/// One task in both reports; a side is `None` when that report did not run the task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskDelta {
    pub task_id: String,
    pub a: Option<Side>,
    pub b: Option<Side>,
    /// `b - a`, only when both ran the task.
    pub pass_rate_delta: Option<f64>,
    pub score_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub a: ReportRef,
    pub b: ReportRef,
    pub tasks: Vec<TaskDelta>,
    pub total_a: Side,
    pub total_b: Side,
    pub pass_rate_delta: f64,
    pub score_delta: f64,
}

fn side(attempts: &[&crate::report::TaskResult]) -> Side {
    let n = attempts.len() as f64;
    if attempts.is_empty() {
        return Side::default();
    }
    let passed = attempts.iter().filter(|t| t.passed).count() as u32;
    Side {
        attempts: attempts.len() as u32,
        passed,
        pass_rate: passed as f64 / n,
        score: attempts.iter().map(|t| t.score).sum::<f64>() / n,
        avg_tokens: attempts.iter().map(|t| t.tokens as f64).sum::<f64>() / n,
        avg_duration_ms: attempts.iter().map(|t| t.duration_ms as f64).sum::<f64>() / n,
    }
}

fn by_task(report: &EvalReport) -> BTreeMap<&str, Side> {
    let mut groups: BTreeMap<&str, Vec<&crate::report::TaskResult>> = BTreeMap::new();
    for t in &report.tasks {
        groups.entry(t.task_id.as_str()).or_default().push(t);
    }
    groups.into_iter().map(|(id, v)| (id, side(&v))).collect()
}

fn reference(r: &EvalReport) -> ReportRef {
    ReportRef { id: r.id.clone(), suite_id: r.suite_id.clone(), agent: r.agent.clone(), profile: r.profile.clone() }
}

/// Compares `b` against `a`: the deltas are `b - a`. Tasks come in the order of `a`, then the ones only `b` has.
pub fn compare(a: &EvalReport, b: &EvalReport) -> Comparison {
    let (sa, sb) = (by_task(a), by_task(b));
    let mut order: Vec<&str> = vec![];
    for r in [a, b] {
        for t in &r.tasks {
            if !order.contains(&t.task_id.as_str()) {
                order.push(&t.task_id);
            }
        }
    }
    let tasks = order
        .into_iter()
        .map(|id| {
            let (x, y) = (sa.get(id).cloned(), sb.get(id).cloned());
            let (pass_rate_delta, score_delta) = match (&x, &y) {
                (Some(x), Some(y)) => (Some(y.pass_rate - x.pass_rate), Some(y.score - x.score)),
                _ => (None, None),
            };
            TaskDelta { task_id: id.to_string(), a: x, b: y, pass_rate_delta, score_delta }
        })
        .collect();
    let total = |r: &EvalReport| side(&r.tasks.iter().collect::<Vec<_>>());
    let (total_a, total_b) = (total(a), total(b));
    Comparison {
        a: reference(a),
        b: reference(b),
        tasks,
        pass_rate_delta: total_b.pass_rate - total_a.pass_rate,
        score_delta: total_b.score - total_a.score,
        total_a,
        total_b,
    }
}

fn label(r: &ReportRef) -> String {
    let who = [r.agent.as_deref(), r.profile.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("/");
    if who.is_empty() {
        r.id.clone()
    } else {
        format!("{} ({who})", r.id)
    }
}

fn pct(v: f64) -> String {
    format!("{:.0}%", v * 100.0)
}

fn signed(v: f64, scale: f64, unit: &str) -> String {
    format!("{:+.0}{unit}", v * scale)
}

fn cell(s: &Option<Side>) -> String {
    match s {
        Some(s) => format!("{}/{} {} s={:.2}", s.passed, s.attempts, pct(s.pass_rate), s.score),
        None => "-".into(),
    }
}

impl Comparison {
    /// A readable table.
    pub fn to_text(&self) -> String {
        let mut out = format!("A: {}\nB: {}\n\n", label(&self.a), label(&self.b));
        let width = self.tasks.iter().map(|t| t.task_id.chars().count()).max().unwrap_or(4).max(6);
        out.push_str(&format!("{:<width$}  {:<20}  {:<20}  {:>8}  {:>8}\n", "task", "A", "B", "Δ riusc.", "Δ score"));
        for t in &self.tasks {
            let (dp, ds) = match (t.pass_rate_delta, t.score_delta) {
                (Some(p), Some(s)) => (signed(p, 100.0, "%"), format!("{s:+.2}")),
                _ => ("-".into(), "-".into()),
            };
            out.push_str(&format!("{:<width$}  {:<20}  {:<20}  {:>8}  {:>8}\n", t.task_id, cell(&t.a), cell(&t.b), dp, ds));
        }
        out.push_str(&format!(
            "{:<width$}  {:<20}  {:<20}  {:>8}  {:>8}\n",
            "TOTALE",
            cell(&Some(self.total_a.clone())),
            cell(&Some(self.total_b.clone())),
            signed(self.pass_rate_delta, 100.0, "%"),
            format!("{:+.2}", self.score_delta),
        ));
        out.push_str(&format!(
            "\ntoken medi: {:.0} -> {:.0}; durata media: {:.1}s -> {:.1}s\n",
            self.total_a.avg_tokens,
            self.total_b.avg_tokens,
            self.total_a.avg_duration_ms / 1000.0,
            self.total_b.avg_duration_ms / 1000.0,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::TaskResult;

    fn task(id: &str, passed: bool, score: f64, tokens: u64) -> TaskResult {
        TaskResult {
            task_id: id.into(),
            attempt: 1,
            passed,
            score,
            graders: vec![],
            duration_ms: 2000,
            tokens,
            turns: 1,
            run_id: None,
            error: None,
            workdir: None,
        }
    }

    fn report(id: &str, profile: &str, tasks: Vec<TaskResult>) -> EvalReport {
        let mut r = EvalReport {
            id: id.into(),
            suite_id: "s".into(),
            suite_name: "S".into(),
            status: "completed".into(),
            started_at: "t".into(),
            finished_at: None,
            agent: None,
            profile: Some(profile.into()),
            repeat: 1,
            tasks,
            summary: Default::default(),
            error: None,
        };
        r.refresh_summary();
        r
    }

    #[test]
    fn deltas_are_b_minus_a_and_cover_tasks_of_both() {
        let a = report("evr_a", "lento", vec![task("uno", true, 1.0, 100), task("due", false, 0.5, 300), task("solo-a", true, 1.0, 10)]);
        let b = report("evr_b", "veloce", vec![task("uno", true, 1.0, 50), task("due", true, 1.0, 100), task("solo-b", false, 0.0, 10)]);
        let c = compare(&a, &b);
        assert_eq!(c.tasks.iter().map(|t| t.task_id.as_str()).collect::<Vec<_>>(), vec!["uno", "due", "solo-a", "solo-b"]);
        let due = &c.tasks[1];
        assert_eq!((due.pass_rate_delta, due.score_delta), (Some(1.0), Some(0.5)));
        assert_eq!(c.tasks[0].pass_rate_delta, Some(0.0));
        assert!(c.tasks[2].b.is_none() && c.tasks[2].pass_rate_delta.is_none());
        assert!(c.tasks[3].a.is_none());
        assert!((c.total_a.pass_rate - 2.0 / 3.0).abs() < 1e-9 && (c.total_b.pass_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(c.pass_rate_delta, 0.0);
        assert!((c.total_a.avg_tokens - 410.0 / 3.0).abs() < 1e-9);
        let text = c.to_text();
        assert!(text.contains("A: evr_a (lento)") && text.contains("B: evr_b (veloce)"), "{text}");
        assert!(text.contains("+100%") && text.contains("+0.50") && text.contains("TOTALE"), "{text}");
    }

    #[test]
    fn repeats_are_averaged_per_task() {
        let a = report("a", "p", vec![task("uno", true, 1.0, 10), task("uno", false, 0.0, 30)]);
        let c = compare(&a, &a);
        let s = c.tasks[0].a.as_ref().unwrap();
        assert_eq!((s.attempts, s.passed, s.pass_rate, s.score, s.avg_tokens), (2, 1, 0.5, 0.5, 20.0));
    }
}
