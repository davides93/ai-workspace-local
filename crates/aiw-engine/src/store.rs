//! SQLite persistence: documents (agents, profiles, workflows, MCP servers, settings), sessions,
//! messages, run events (replay), audit trail and workflow runs.

use aiw_core::api::{AuditEntry, ChatMessage, Feedback, Session, WorkflowRun};
use aiw_core::events::SeqEvent;
use aiw_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// One row of the `runs` table.
#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

fn db<E: std::fmt::Display>(e: E) -> Error {
    Error::Other(format!("errore del database: {e}"))
}

fn feedback_row(r: &rusqlite::Row) -> rusqlite::Result<Feedback> {
    Ok(Feedback {
        id: r.get(0)?,
        session_id: r.get(1)?,
        message_id: r.get(2)?,
        run_id: r.get(3)?,
        rating: r.get(4)?,
        note: r.get(5)?,
        created_at: r.get(6)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(db)?;
        Self::init(conn)
    }
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db)?)
    }
    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS documents (kind TEXT NOT NULL, id TEXT NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(kind, id));
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL, agent_id TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_status TEXT);
             CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, seq INTEGER NOT NULL, json TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id, seq);
             CREATE TABLE IF NOT EXISTS events (run_id TEXT NOT NULL, seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(run_id, seq));
             CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, session_id TEXT, status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT);
             CREATE TABLE IF NOT EXISTS audit (id TEXT PRIMARY KEY, ts TEXT NOT NULL, json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS feedback (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, message_id TEXT NOT NULL, run_id TEXT, rating INTEGER NOT NULL, note TEXT, created_at TEXT NOT NULL);
             CREATE UNIQUE INDEX IF NOT EXISTS feedback_message ON feedback(session_id, message_id);
             CREATE TABLE IF NOT EXISTS eval_runs (id TEXT PRIMARY KEY, suite_id TEXT NOT NULL, started_at TEXT NOT NULL, json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS train_jobs (id TEXT PRIMARY KEY, kind TEXT NOT NULL, trainer TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, json TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS profile_rewards (agent_id TEXT NOT NULL, key TEXT NOT NULL, profile_id TEXT NOT NULL, reward REAL NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(agent_id, key));
             CREATE TABLE IF NOT EXISTS workflow_runs (id TEXT PRIMARY KEY, workflow_id TEXT NOT NULL, started_at TEXT NOT NULL, json TEXT NOT NULL);",
        )
        .map_err(db)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    // ---------------- documents
    pub fn put_doc<T: Serialize>(&self, kind: &str, id: &str, v: &T) -> Result<()> {
        let json = serde_json::to_string(v)?;
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO documents(kind,id,json,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET json=excluded.json, updated_at=excluded.updated_at", params![kind, id, json, aiw_core::now_rfc3339()])
            .map_err(db)?;
        Ok(())
    }
    pub fn get_doc<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<Option<T>> {
        let s: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT json FROM documents WHERE kind=?1 AND id=?2", params![kind, id], |r| r.get(0))
            .optional()
            .map_err(db)?;
        s.map(|s| serde_json::from_str(&s).map_err(Error::from)).transpose()
    }
    pub fn list_docs<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM documents WHERE kind=?1 ORDER BY rowid").map_err(db)?;
        let rows = st.query_map(params![kind], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            match serde_json::from_str(&r.map_err(db)?) {
                Ok(v) => out.push(v),
                Err(e) => tracing::warn!(kind, "skipping invalid document: {e}"),
            }
        }
        Ok(out)
    }
    pub fn delete_doc(&self, kind: &str, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute("DELETE FROM documents WHERE kind=?1 AND id=?2", params![kind, id]).map_err(db)? > 0)
    }

    // ---------------- sessions & messages
    pub fn upsert_session(&self, s: &Session) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO sessions(id,title,agent_id,created_at,updated_at,last_status) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(id) DO UPDATE SET title=excluded.title, agent_id=excluded.agent_id, updated_at=excluded.updated_at, last_status=excluded.last_status",
                params![s.id, s.title, s.agent_id, s.created_at, s.updated_at, s.last_status],
            )
            .map_err(db)?;
        Ok(())
    }
    /// Changes only the title: renaming is not activity, so `updated_at` (the sort key) stays.
    pub fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        let n = self.conn.lock().unwrap().execute("UPDATE sessions SET title=?2 WHERE id=?1", params![id, title]).map_err(db)?;
        if n == 0 {
            return Err(Error::NotFound(format!("sessione {id} non trovata")));
        }
        Ok(())
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT id,title,agent_id,created_at,updated_at,last_status FROM sessions WHERE id=?1", params![id], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    agent_id: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                    last_status: r.get(5)?,
                    worktree: None,
                })
            })
            .optional()
            .map_err(db)?
            .ok_or_else(|| Error::NotFound(format!("sessione {id} non trovata")))
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT id,title,agent_id,created_at,updated_at,last_status FROM sessions ORDER BY updated_at DESC LIMIT 500")
            .map_err(db)?;
        let rows = st
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    agent_id: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                    last_status: r.get(5)?,
                    worktree: None,
                })
            })
            .map_err(db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db)
    }
    /// Every session, most recently active first (no cap, unlike [`Store::sessions`]): for exports.
    pub fn all_sessions(&self) -> Result<Vec<Session>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT id,title,agent_id,created_at,updated_at,last_status FROM sessions ORDER BY updated_at DESC").map_err(db)?;
        let rows = st
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    agent_id: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                    last_status: r.get(5)?,
                    worktree: None,
                })
            })
            .map_err(db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db)
    }
    pub fn delete_session(&self, id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM messages WHERE session_id=?1", params![id]).map_err(db)?;
        c.execute("DELETE FROM sessions WHERE id=?1", params![id]).map_err(db)?;
        Ok(())
    }
    pub fn append_messages(&self, session_id: &str, msgs: &[ChatMessage]) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db)?;
        let base: i64 = tx
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM messages WHERE session_id=?1", params![session_id], |r| r.get(0))
            .map_err(db)?;
        for (i, m) in msgs.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO messages(id,session_id,seq,json) VALUES(?1,?2,?3,?4)",
                params![m.id, session_id, base + 1 + i as i64, serde_json::to_string(m)?],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }
    /// Rewrites a stored message in place (same id and position).
    pub fn replace_message(&self, session_id: &str, m: &ChatMessage) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE messages SET json=?3 WHERE id=?1 AND session_id=?2", params![m.id, session_id, serde_json::to_string(m)?])
            .map_err(db)?;
        Ok(())
    }
    pub fn messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM messages WHERE session_id=?1 ORDER BY seq").map_err(db)?;
        let rows = st.query_map(params![session_id], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }

    // ---------------- feedback
    /// Sets the rating of a message; a second call for the same message replaces the first
    /// (the row keeps its id).
    pub fn upsert_feedback(&self, f: &Feedback) -> Result<Feedback> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO feedback(id,session_id,message_id,run_id,rating,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(session_id,message_id) DO UPDATE SET run_id=excluded.run_id, rating=excluded.rating, note=excluded.note, created_at=excluded.created_at",
            params![f.id, f.session_id, f.message_id, f.run_id, f.rating, f.note, f.created_at],
        )
        .map_err(db)?;
        c.query_row(
            "SELECT id,session_id,message_id,run_id,rating,note,created_at FROM feedback WHERE session_id=?1 AND message_id=?2",
            params![f.session_id, f.message_id],
            feedback_row,
        )
        .map_err(db)
    }
    pub fn delete_feedback(&self, session_id: &str, message_id: &str) -> Result<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM feedback WHERE session_id=?1 AND message_id=?2", params![session_id, message_id])
            .map_err(db)?;
        Ok(n > 0)
    }
    /// The ratings of a session, in the order they were given.
    pub fn session_feedback(&self, session_id: &str) -> Result<Vec<Feedback>> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare("SELECT id,session_id,message_id,run_id,rating,note,created_at FROM feedback WHERE session_id=?1 ORDER BY created_at, rowid")
            .map_err(db)?;
        let rows = st.query_map(params![session_id], feedback_row).map_err(db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db)
    }

    // ---------------- runs & events
    pub fn start_run(&self, id: &str, session_id: Option<&str>) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO runs(id,session_id,status,started_at) VALUES(?1,?2,'running',?3)",
                params![id, session_id, aiw_core::now_rfc3339()],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn finish_run(&self, id: &str, status: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE runs SET status=?2, finished_at=?3 WHERE id=?1", params![id, status, aiw_core::now_rfc3339()])
            .map_err(db)?;
        Ok(())
    }
    /// Runs still marked `running` when the process starts were interrupted by a crash or a kill:
    /// mark them failed (and the sessions that were running them), so nothing waits forever.
    pub fn fail_interrupted_runs(&self) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        let n = c
            .execute("UPDATE runs SET status='failed', finished_at=?1 WHERE status='running'", params![aiw_core::now_rfc3339()])
            .map_err(db)?;
        Ok(n)
    }
    /// The top-level runs of a session, oldest first.
    pub fn session_runs(&self, session_id: &str) -> Result<Vec<RunRow>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT id,status,started_at,finished_at FROM runs WHERE session_id=?1 ORDER BY started_at, rowid").map_err(db)?;
        let rows = st
            .query_map(params![session_id], |r| {
                Ok(RunRow { id: r.get(0)?, status: r.get(1)?, started_at: r.get(2)?, finished_at: r.get(3)? })
            })
            .map_err(db)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(db)
    }
    pub fn save_events(&self, run_id: &str, evs: &[SeqEvent]) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(db)?;
        for e in evs {
            tx.execute(
                "INSERT OR REPLACE INTO events(run_id,seq,json) VALUES(?1,?2,?3)",
                params![run_id, e.seq as i64, serde_json::to_string(&e.event)?],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }
    pub fn events(&self, run_id: &str) -> Result<Vec<SeqEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT seq,json FROM events WHERE run_id=?1 ORDER BY seq").map_err(db)?;
        let rows = st.query_map(params![run_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            let (seq, json) = r.map_err(db)?;
            out.push(SeqEvent { seq: seq as u64, event: serde_json::from_str(&json)? });
        }
        Ok(out)
    }

    // ---------------- audit & workflow runs
    pub fn add_audit(&self, e: &AuditEntry) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("INSERT OR REPLACE INTO audit(id,ts,json) VALUES(?1,?2,?3)", params![e.id, e.ts, serde_json::to_string(e)?])
            .map_err(db)?;
        Ok(())
    }
    pub fn audit(&self, limit: usize) -> Result<Vec<AuditEntry>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM audit ORDER BY ts DESC, rowid DESC LIMIT ?1").map_err(db)?;
        let rows = st.query_map(params![limit as i64], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }
    /// The audit entries of one run, oldest first.
    pub fn audit_for_run(&self, run_id: &str) -> Result<Vec<AuditEntry>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM audit WHERE json_extract(json,'$.run_id')=?1 ORDER BY ts, rowid").map_err(db)?;
        let rows = st.query_map(params![run_id], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }
    // ---------------- evaluation reports
    pub fn save_eval_report<T: Serialize>(&self, id: &str, suite_id: &str, started_at: &str, report: &T) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO eval_runs(id,suite_id,started_at,json) VALUES(?1,?2,?3,?4)",
                params![id, suite_id, started_at, serde_json::to_string(report)?],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn eval_report<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let s: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT json FROM eval_runs WHERE id=?1", params![id], |r| r.get(0))
            .optional()
            .map_err(db)?;
        s.map(|s| serde_json::from_str(&s).map_err(Error::from)).transpose()
    }
    /// The most recent reports first.
    pub fn eval_reports<T: DeserializeOwned>(&self, limit: usize) -> Result<Vec<T>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM eval_runs ORDER BY started_at DESC, rowid DESC LIMIT ?1").map_err(db)?;
        let rows = st.query_map(params![limit as i64], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }
    // ---------------- bandit router
    /// Sets the reward of `key` (a thumb, an evaluation attempt) for a profile of an agent; the same key
    /// again replaces it, so a changed thumb is counted once.
    pub fn set_profile_reward(&self, agent_id: &str, profile_id: &str, key: &str, reward: f64) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO profile_rewards(agent_id,key,profile_id,reward,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![agent_id, key, profile_id, reward, aiw_core::now_rfc3339()],
            )
            .map_err(db)?;
        Ok(())
    }
    /// Removes the reward of `key` (a thumb taken back).
    pub fn delete_profile_reward(&self, key: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM profile_rewards WHERE key=?1", params![key]).map_err(db)?;
        Ok(())
    }
    /// `(profile, n, reward_sum)` of an agent.
    pub fn profile_rewards(&self, agent_id: &str) -> Result<Vec<(String, u64, f64)>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT profile_id, COUNT(*), SUM(reward) FROM profile_rewards WHERE agent_id=?1 GROUP BY profile_id").map_err(db)?;
        let rows = st
            .query_map(params![agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, f64>(2)?)))
            .map_err(db)?;
        rows.map(|r| r.map_err(db)).collect()
    }
    pub fn reset_profile_rewards(&self, agent_id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM profile_rewards WHERE agent_id=?1", params![agent_id]).map_err(db)?;
        Ok(())
    }

    // ---------------- fine-tuning jobs
    /// Inserts or replaces a job; `kind` is the method (`sft`, `dpo`, `grpo`).
    pub fn save_train_job<T: Serialize>(&self, id: &str, kind: &str, trainer: &str, status: &str, created_at: &str, job: &T) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO train_jobs(id,kind,trainer,status,created_at,json) VALUES(?1,?2,?3,?4,?5,?6)",
                params![id, kind, trainer, status, created_at, serde_json::to_string(job)?],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn train_job<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let s: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT json FROM train_jobs WHERE id=?1", params![id], |r| r.get(0))
            .optional()
            .map_err(db)?;
        s.map(|s| serde_json::from_str(&s).map_err(Error::from)).transpose()
    }
    /// The most recent jobs first.
    pub fn train_jobs<T: DeserializeOwned>(&self, limit: usize) -> Result<Vec<T>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM train_jobs ORDER BY created_at DESC, rowid DESC LIMIT ?1").map_err(db)?;
        let rows = st.query_map(params![limit as i64], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }
    pub fn delete_train_job(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute("DELETE FROM train_jobs WHERE id=?1", params![id]).map_err(db)? > 0)
    }

    pub fn save_workflow_run(&self, r: &WorkflowRun) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO workflow_runs(id,workflow_id,started_at,json) VALUES(?1,?2,?3,?4)",
                params![r.id, r.workflow_id, r.started_at, serde_json::to_string(r)?],
            )
            .map_err(db)?;
        Ok(())
    }
    pub fn workflow_runs(&self, limit: usize) -> Result<Vec<WorkflowRun>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json FROM workflow_runs ORDER BY started_at DESC LIMIT ?1").map_err(db)?;
        let rows = st.query_map(params![limit as i64], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r.map_err(db)?)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::events::{AgEvent, TimedEvent};

    #[test]
    fn roundtrips_everything() {
        let s = Store::in_memory().unwrap();
        s.put_doc("agent", "a", &serde_json::json!({"x": 1})).unwrap();
        s.put_doc("agent", "a", &serde_json::json!({"x": 2})).unwrap();
        assert_eq!(s.list_docs::<serde_json::Value>("agent").unwrap(), vec![serde_json::json!({"x": 2})]);
        let ses = Session {
            id: "ses".into(),
            title: "T".into(),
            agent_id: "a".into(),
            created_at: "1".into(),
            updated_at: "1".into(),
            last_status: None,
            worktree: None,
        };
        s.upsert_session(&ses).unwrap();
        let m = |id: &str| ChatMessage {
            id: id.into(),
            role: "user".into(),
            content: id.into(),
            reasoning: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            created_at: "1".into(),
        };
        s.append_messages("ses", &[m("1"), m("2")]).unwrap();
        s.append_messages("ses", &[m("3")]).unwrap();
        assert_eq!(s.messages("ses").unwrap().iter().map(|m| m.id.clone()).collect::<Vec<_>>(), vec!["1", "2", "3"]);
        s.save_events("r", &[SeqEvent { seq: 0, event: TimedEvent::now(AgEvent::StepStarted { step_name: "x".into() }) }]).unwrap();
        assert_eq!(s.events("r").unwrap().len(), 1);
        s.delete_session("ses").unwrap();
        assert!(s.messages("ses").unwrap().is_empty());
    }
}
