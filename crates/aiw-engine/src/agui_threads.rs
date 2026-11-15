//! The durable side of the AG-UI endpoint (`POST /agui/{agent}`, `docs/agui.md`): which session a client
//! `threadId` belongs to, which client message started which run, and which id a user message has on
//! the client. It lives in the `documents` table (kind `agui_thread`, one row per thread), so it survives
//! a restart, and it goes away with the session.
//!
//! What it holds is ids only: no message text, no state, no context.

use crate::Engine;
use aiw_core::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Kind of the document rows.
const KIND: &str = "agui_thread";
/// Runs kept per thread (the oldest are forgotten: a retry of a run that old starts a new one).
const MAX_RUNS: usize = 64;
/// User message ids kept per thread.
const MAX_USER_IDS: usize = 2000;

/// A run that a client message started on a thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AguiRun {
    /// The `runId` the client sent.
    pub client_run_id: String,
    /// The run of the engine (the one at `/api/runs/{id}/events`).
    pub run_id: String,
    /// The id the client gave to the user message that started the run.
    pub user_message_id: String,
    /// The id the session gave to the same message.
    pub user_server_id: String,
}

/// What the server knows of one client thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AguiThread {
    pub thread_id: String,
    pub session_id: String,
    pub agent_id: String,
    /// The runs the thread started through this endpoint, the oldest first.
    #[serde(default)]
    pub runs: Vec<AguiRun>,
    /// Id of a user message in the session → the id the client gave it.
    #[serde(default)]
    pub user_ids: BTreeMap<String, String>,
}

impl AguiThread {
    pub fn new(thread_id: &str, session_id: &str, agent_id: &str) -> Self {
        Self { thread_id: thread_id.into(), session_id: session_id.into(), agent_id: agent_id.into(), ..Default::default() }
    }

    /// The run a client message started, if it did.
    pub fn run_of_message(&self, client_message_id: &str) -> Option<&AguiRun> {
        self.runs.iter().rev().find(|r| r.user_message_id == client_message_id)
    }

    /// The run the client asked for with this `runId`, if it did.
    pub fn run_of_client_run(&self, client_run_id: &str) -> Option<&AguiRun> {
        self.runs.iter().rev().find(|r| r.client_run_id == client_run_id)
    }

    /// Is this id the id a client gave to one of the user messages of the thread?
    pub fn is_client_message(&self, client_message_id: &str) -> bool {
        self.user_ids.values().any(|v| v == client_message_id)
    }

    /// Records a run and the ids of its user message.
    pub fn record(&mut self, run: AguiRun) {
        // A message the client sent twice (a retry after a run that failed) is two messages in the session: only the
        // first keeps the client's id, so that ids stay unique in a snapshot.
        if !self.is_client_message(&run.user_message_id) {
            self.user_ids.insert(run.user_server_id.clone(), run.user_message_id.clone());
        }
        self.runs.push(run);
        if self.runs.len() > MAX_RUNS {
            let extra = self.runs.len() - MAX_RUNS;
            self.runs.drain(..extra);
        }
        while self.user_ids.len() > MAX_USER_IDS {
            // Server ids are ordered by time: the first key is the oldest.
            let Some(first) = self.user_ids.keys().next().cloned() else { break };
            self.user_ids.remove(&first);
        }
    }
}

impl Engine {
    /// The record of a thread, `None` for a thread never seen.
    pub fn agui_thread(&self, thread_id: &str) -> Result<Option<AguiThread>> {
        self.0.store.get_doc(KIND, thread_id)
    }

    pub fn save_agui_thread(&self, t: &AguiThread) -> Result<()> {
        self.0.store.put_doc(KIND, &t.thread_id, t)
    }

    pub fn remove_agui_thread(&self, thread_id: &str) -> Result<()> {
        self.0.store.delete_doc(KIND, thread_id).map(|_| ())
    }

    /// Forgets the threads of a deleted session.
    pub(crate) fn remove_agui_threads_of(&self, session_id: &str) {
        if let Ok(all) = self.0.store.list_docs::<AguiThread>(KIND) {
            for t in all.into_iter().filter(|t| t.session_id == session_id) {
                let _ = self.0.store.delete_doc(KIND, &t.thread_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_remembers_runs_and_forgets_the_oldest() {
        let mut t = AguiThread::new("thr", "ses_1", "planner");
        for i in 0..70 {
            t.record(AguiRun {
                client_run_id: format!("r{i}"),
                run_id: format!("run_{i}"),
                user_message_id: format!("m{i}"),
                user_server_id: format!("msg_{i:04}"),
            });
        }
        assert_eq!(t.runs.len(), MAX_RUNS);
        assert!(t.run_of_client_run("r0").is_none() && t.run_of_client_run("r69").is_some());
        assert_eq!(t.run_of_message("m69").map(|r| r.run_id.as_str()), Some("run_69"));
        assert!(t.is_client_message("m3") && !t.is_client_message("zzz"));
    }

    #[tokio::test]
    async fn the_record_is_stored_and_goes_with_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let e = crate::Engine::new(crate::EngineOptions {
            data_dir: Some(dir.path().into()),
            workspace_root: Some(ws.path().into()),
            mode: "daemon".into(),
            offline: true,
            ..Default::default()
        })
        .await
        .unwrap();
        let ses = e.create_session("planner", Some("x".into())).unwrap();
        let other = e.create_session("planner", Some("y".into())).unwrap();
        assert!(e.agui_thread("thr").unwrap().is_none());
        let mut t = AguiThread::new("thr", &ses.id, "planner");
        t.record(AguiRun { client_run_id: "r1".into(), run_id: "run_1".into(), user_message_id: "m1".into(), user_server_id: "msg_1".into() });
        e.save_agui_thread(&t).unwrap();
        e.save_agui_thread(&AguiThread::new("thr-2", &other.id, "planner")).unwrap();
        assert_eq!(e.agui_thread("thr").unwrap(), Some(t));
        e.delete_session(&ses.id).unwrap();
        assert!(e.agui_thread("thr").unwrap().is_none(), "the thread goes with its session");
        assert!(e.agui_thread("thr-2").unwrap().is_some(), "another session's thread stays");
    }
}
