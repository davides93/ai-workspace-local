//! What deserves a desktop notification: an approval waiting, a run that ended, a download that
//! finished. Pure (no windowing code), so the Tauri shell only decides *whether* to show a
//! [`Notice`] (setting on, window out of sight) and shows it.
//!
//! The shell feeds it the run-event bus ([`NoticeTracker::on_event`]) and the download list
//! ([`NoticeTracker::on_downloads`], downloads publish no event: they are polled).

use crate::RunEventMsg;
use aiw_core::api::{ApprovalRequest, DownloadProgress, Proposal};
use aiw_core::events::AgEvent;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Approval,
    RunFinished,
    RunFailed,
    Download,
    DownloadFailed,
    /// The assistant noticed something worth proposing (a new open proposal or the morning briefing).
    Proposal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub title: String,
    pub body: String,
}

fn short(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    let cut: String = one_line.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[derive(Debug, Default)]
pub struct NoticeTracker {
    /// Runs started by a sub-agent: their end is part of the parent's, not news.
    sub_runs: HashSet<String>,
    /// Status of every download seen; `None` until the first poll (what was already done at start is not news).
    downloads: Option<HashMap<String, String>>,
    /// Proposals already announced (or already there at start); `None` until the first poll.
    proposals: Option<HashSet<String>>,
}

impl NoticeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, msg: &RunEventMsg) -> Option<Notice> {
        match &msg.event.event {
            AgEvent::SubagentStarted { run_id: Some(id), .. } => {
                self.sub_runs.insert(id.clone());
                None
            }
            AgEvent::Custom { name, value } if name == "aiw.approval_request" => {
                let req: ApprovalRequest = serde_json::from_value(value.clone()).ok()?;
                let body = if req.reason.trim().is_empty() {
                    format!("Un assistente vuole usare «{}»: apri l'app per rispondere.", req.tool)
                } else {
                    short(&req.reason, 140)
                };
                Some(Notice { kind: NoticeKind::Approval, title: "Serve la tua conferma".into(), body })
            }
            AgEvent::RunFinished { .. } if !self.sub_runs.remove(&msg.run_id) => Some(Notice {
                kind: NoticeKind::RunFinished,
                title: "Lavoro finito".into(),
                body: "Un assistente ha finito: apri l'app per vedere il risultato.".into(),
            }),
            AgEvent::RunError { message, .. } if !self.sub_runs.remove(&msg.run_id) => {
                Some(Notice { kind: NoticeKind::RunFailed, title: "Lavoro interrotto".into(), body: short(message, 140) })
            }
            _ => None,
        }
    }

    /// Open proposals the person has not been told about. During quiet hours nothing is said and nothing is
    /// marked as told, so the notification comes when they end. What was already open at start is not news.
    pub fn on_proposals(&mut self, open: &[Proposal], quiet: bool) -> Vec<Notice> {
        let Some(told) = self.proposals.as_mut() else {
            self.proposals = Some(open.iter().map(|p| p.id.clone()).collect());
            return vec![];
        };
        if quiet {
            return vec![];
        }
        let mut out = vec![];
        for p in open.iter().filter(|p| p.status == "open") {
            if !told.insert(p.id.clone()) {
                continue;
            }
            out.push(Notice {
                kind: NoticeKind::Proposal,
                title: if p.kind == "briefing" { "Il riepilogo di oggi".into() } else { "Ho notato una cosa".into() },
                body: short(if p.noticed.trim().is_empty() { &p.title } else { &p.noticed }, 140),
            });
        }
        out
    }

    pub fn on_downloads(&mut self, list: &[DownloadProgress]) -> Vec<Notice> {
        let now: HashMap<String, String> = list.iter().map(|d| (d.id.clone(), d.status.clone())).collect();
        let Some(before) = self.downloads.replace(now) else { return vec![] };
        list.iter()
            .filter(|d| {
                before.get(&d.id).is_some_and(|s| *s != d.status)
                    || (!before.contains_key(&d.id) && d.status != "queued" && d.status != "running" && d.status != "verifying")
            })
            .filter_map(|d| {
                let name = if d.model_ref.is_empty() { d.model_id.as_str() } else { d.model_ref.as_str() };
                match d.status.as_str() {
                    "done" => Some(Notice {
                        kind: NoticeKind::Download,
                        title: "Download completato".into(),
                        body: format!("{name} è pronto da usare."),
                    }),
                    "error" => Some(Notice {
                        kind: NoticeKind::DownloadFailed,
                        title: "Download non riuscito".into(),
                        body: short(&format!("{name}: {}", d.error.as_deref().unwrap_or("errore sconosciuto")), 140),
                    }),
                    _ => None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::events::TimedEvent;
    use serde_json::json;

    fn msg(run: &str, ev: AgEvent) -> RunEventMsg {
        RunEventMsg { run_id: run.into(), seq: 0, event: TimedEvent::now(ev) }
    }
    fn dl(id: &str, status: &str) -> DownloadProgress {
        DownloadProgress {
            id: id.into(),
            model_id: "qwen3-4b".into(),
            model_ref: "Qwen3 4B".into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_approval_request_says_what_is_asked() {
        let mut t = NoticeTracker::new();
        let req = ApprovalRequest {
            id: "a1".into(),
            run_id: "r1".into(),
            tool: "shell".into(),
            args: json!({}),
            reason: "Eseguire `rm -rf build`".into(),
            risk: "high".into(),
        };
        let n = t.on_event(&msg("r1", AgEvent::approval_request(&req))).unwrap();
        assert_eq!((n.kind, n.title.as_str(), n.body.as_str()), (NoticeKind::Approval, "Serve la tua conferma", "Eseguire `rm -rf build`"));
        let bare = ApprovalRequest { reason: " ".into(), ..req };
        assert!(t.on_event(&msg("r1", AgEvent::approval_request(&bare))).unwrap().body.contains("«shell»"));
        // Resolutions are not news.
        assert!(t.on_event(&msg("r1", AgEvent::approval_resolved("a1", true))).is_none());
    }

    #[test]
    fn a_run_end_is_news_but_a_subagents_is_not() {
        let mut t = NoticeTracker::new();
        let fin = |r: &str| msg(r, AgEvent::RunFinished { thread_id: "s".into(), run_id: r.into(), result: None });
        assert_eq!(t.on_event(&fin("r1")).unwrap().kind, NoticeKind::RunFinished);
        t.on_event(&msg("r2", AgEvent::SubagentStarted { subagent_id: "x".into(), name: "n".into(), run_id: Some("sub1".into()) }));
        assert!(t.on_event(&fin("sub1")).is_none());
        assert!(t.sub_runs.is_empty());
        let err = t.on_event(&msg("r3", AgEvent::RunError { message: "Il modello non risponde\ndavvero".into(), code: None })).unwrap();
        assert_eq!((err.kind, err.body.as_str()), (NoticeKind::RunFailed, "Il modello non risponde davvero"));
        assert!(t.on_event(&msg("r3", AgEvent::StepStarted { step_name: "x".into() })).is_none());
    }

    #[test]
    fn long_text_is_cut_to_one_short_line() {
        let long = "parola ".repeat(60);
        let s = short(&long, 40);
        assert!(s.chars().count() <= 40 && s.ends_with('…'), "{s}");
    }

    fn prop(id: &str, kind: &str) -> Proposal {
        Proposal {
            id: id.into(),
            kind: kind.into(),
            title: "Titolo".into(),
            noticed: "Sono arrivate due fatture.".into(),
            status: "open".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_new_proposal_is_news_once_and_quiet_hours_hold_it_back() {
        let mut t = NoticeTracker::new();
        // Already open at start: not announced.
        assert!(t.on_proposals(&[prop("p0", "proposal")], false).is_empty());
        let list = [prop("p0", "proposal"), prop("p1", "proposal"), prop("b1", "briefing")];
        // In quiet hours: silence, and nothing is marked as told.
        assert!(t.on_proposals(&list, true).is_empty());
        let n = t.on_proposals(&list, false);
        assert_eq!(n.len(), 2);
        assert_eq!(
            (n[0].kind, n[0].title.as_str(), n[0].body.as_str()),
            (NoticeKind::Proposal, "Ho notato una cosa", "Sono arrivate due fatture.")
        );
        assert_eq!(n[1].title, "Il riepilogo di oggi");
        assert!(t.on_proposals(&list, false).is_empty(), "said once");
    }

    #[test]
    fn a_download_is_news_when_it_changes_to_done_or_error() {
        let mut t = NoticeTracker::new();
        // The first poll only records: a download that was already done is not announced at start.
        assert!(t.on_downloads(&[dl("d0", "done"), dl("d1", "running")]).is_empty());
        assert!(t.on_downloads(&[dl("d0", "done"), dl("d1", "running")]).is_empty());
        let n = t.on_downloads(&[dl("d0", "done"), dl("d1", "done")]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].kind, n[0].body.as_str()), (NoticeKind::Download, "Qwen3 4B è pronto da usare."));
        // Said once.
        assert!(t.on_downloads(&[dl("d0", "done"), dl("d1", "done")]).is_empty());
        // A job that appears and fails between two polls, and a cancelled one.
        let mut failed = dl("d2", "error");
        failed.error = Some("Disco pieno".into());
        let n = t.on_downloads(&[dl("d0", "done"), dl("d1", "done"), failed, dl("d3", "cancelled")]);
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].kind, n[0].body.as_str()), (NoticeKind::DownloadFailed, "Qwen3 4B: Disco pieno"));
    }
}
