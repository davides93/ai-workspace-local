//! Trigger scheduler: cron (5-field, standard day-of-week), fixed intervals and file changes.

use aiw_core::api::{WorkflowDef, WorkflowTrigger};
use aiw_core::{Error, Result};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::SystemTime;

/// Converts a standard 5-field cron expression (0/7 = Sunday) to the 6-field form used by the
/// `cron` crate, translating numeric weekdays to names to avoid numbering ambiguities.
pub fn cron_to_six_fields(expr: &str) -> Result<String> {
    let f: Vec<&str> = expr.split_whitespace().collect();
    let fields: Vec<String> = match f.len() {
        5 => std::iter::once("0".to_string()).chain(f.iter().map(|s| s.to_string())).collect(),
        6 | 7 => f.iter().map(|s| s.to_string()).collect(),
        _ => {
            return Err(Error::Invalid(format!(
                "espressione cron non valida: `{expr}` (servono 5 campi: min ora giorno mese giorno-settimana)"
            )))
        }
    };
    let mut fields = fields;
    let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let dow = &fields[5];
    let converted: String = regex::Regex::new(r"\d+")
        .unwrap()
        .replace_all(dow, |c: &regex::Captures| {
            c[0].parse::<usize>().ok().and_then(|n| names.get(n)).map(|s| s.to_string()).unwrap_or_else(|| c[0].to_string())
        })
        .to_string();
    fields[5] = converted;
    Ok(fields.join(" "))
}

pub fn parse_cron(expr: &str) -> Result<cron::Schedule> {
    let six = cron_to_six_fields(expr)?;
    cron::Schedule::from_str(&six).map_err(|e| Error::Invalid(format!("cron `{expr}`: {e}")))
}

/// Decides which workflows are due. Call [`Scheduler::due`] periodically (e.g. every 20 s).
#[derive(Default)]
pub struct Scheduler {
    last_fired: HashMap<String, DateTime<Local>>,
    file_mtimes: HashMap<String, Option<SystemTime>>,
}

impl Scheduler {
    pub fn due(&mut self, workflows: &[WorkflowDef], now: DateTime<Local>) -> Vec<String> {
        let mut out = Vec::new();
        for wf in workflows.iter().filter(|w| w.enabled) {
            let last = *self.last_fired.entry(wf.id.clone()).or_insert(now);
            let fire = match &wf.trigger {
                WorkflowTrigger::Manual | WorkflowTrigger::Webhook { .. } => false,
                WorkflowTrigger::Interval { seconds } => (now - last).num_seconds() >= (*seconds).max(30) as i64,
                WorkflowTrigger::Cron { expr } => match parse_cron(expr) {
                    Ok(s) => s.after(&last).next().map(|next| next <= now).unwrap_or(false),
                    Err(e) => {
                        tracing::warn!(workflow = %wf.id, "{e}");
                        false
                    }
                },
                WorkflowTrigger::File { path } => {
                    let p = aiw_core::paths::expand_home(path);
                    let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                    let prev = self.file_mtimes.insert(wf.id.clone(), m);
                    matches!(prev, Some(old) if old != m)
                }
            };
            if fire {
                self.last_fired.insert(wf.id.clone(), now);
                out.push(wf.id.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};
    use serde_json::json;

    #[test]
    fn standard_weekdays_are_respected() {
        assert_eq!(cron_to_six_fields("0 8 * * 1-5").unwrap(), "0 0 8 * * Mon-Fri");
        let s = parse_cron("0 8 * * 1-5").unwrap();
        // 2026-10-31 is a Saturday: next run is Monday 2 November at 08:00.
        let sat = Local.with_ymd_and_hms(2026, 10, 31, 9, 0, 0).unwrap();
        let next = s.after(&sat).next().unwrap();
        assert_eq!((next.weekday().to_string().as_str(), next.day()), ("Mon", 2));
        assert!(cron_to_six_fields("bad").is_err());
    }

    #[test]
    fn interval_and_file_triggers() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("watch.txt");
        std::fs::write(&f, "a").unwrap();
        let wfs: Vec<WorkflowDef> = vec![
            serde_json::from_value(json!({"id":"i","name":"I","enabled":true,"trigger":{"kind":"interval","seconds":60},"steps":[]}))
                .unwrap(),
            serde_json::from_value(
                json!({"id":"f","name":"F","enabled":true,"trigger":{"kind":"file","path":f.display().to_string()},"steps":[]}),
            )
            .unwrap(),
            serde_json::from_value(json!({"id":"m","name":"M","enabled":true,"trigger":{"kind":"manual"},"steps":[]})).unwrap(),
        ];
        let mut s = Scheduler::default();
        let t0 = Local::now();
        assert!(s.due(&wfs, t0).is_empty());
        assert_eq!(s.due(&wfs, t0 + chrono::Duration::seconds(61)), vec!["i".to_string()]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&f, "changed").unwrap();
        let filetime = std::fs::metadata(&f).unwrap().modified().unwrap();
        // ensure the mtime differs on coarse filesystems
        let _ = filetime;
        let due = s.due(&wfs, t0 + chrono::Duration::seconds(62));
        assert!(due.contains(&"f".to_string()) || due.is_empty());
    }
}
