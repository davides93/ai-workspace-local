//! Storage of the proactive assistant's proposals: the `proposals` table. The migration is
//! `IF NOT EXISTS` and runs on every start: an existing database is left alone and a database
//! from before this table gets it.

use super::{db, Store};
use aiw_core::api::Proposal;
use aiw_core::Result;
use rusqlite::{params, Connection, OptionalExtension};

const COLUMNS: &str =
    "id, kind, title, noticed, instruction, source, agent_id, status, created_at, expires_at, decided_at, run_id, session_id";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Proposal> {
    Ok(Proposal {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        noticed: r.get(3)?,
        instruction: r.get(4)?,
        source: r.get(5)?,
        agent_id: r.get(6)?,
        status: r.get(7)?,
        created_at: r.get(8)?,
        expires_at: r.get(9)?,
        decided_at: r.get(10)?,
        run_id: r.get(11)?,
        session_id: r.get(12)?,
    })
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS proposals (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            noticed TEXT NOT NULL,
            instruction TEXT NOT NULL,
            source TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            decided_at TEXT,
            run_id TEXT,
            session_id TEXT
         );
         CREATE INDEX IF NOT EXISTS proposals_status ON proposals(status, created_at);
         CREATE INDEX IF NOT EXISTS proposals_source ON proposals(source);",
    )
    .map_err(db)
}

impl Store {
    pub fn proposal_insert(&self, p: &Proposal) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                &format!("INSERT INTO proposals({COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
                params![
                    p.id,
                    p.kind,
                    p.title,
                    p.noticed,
                    p.instruction,
                    p.source,
                    p.agent_id,
                    p.status,
                    p.created_at,
                    p.expires_at,
                    p.decided_at,
                    p.run_id,
                    p.session_id
                ],
            )
            .map_err(db)?;
        Ok(())
    }

    pub fn proposal_get(&self, id: &str) -> Result<Option<Proposal>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(&format!("SELECT {COLUMNS} FROM proposals WHERE id=?1"), params![id], row)
            .optional()
            .map_err(db)
    }

    /// Newest first. `status`: only that one; `None` = every status.
    pub fn proposal_list(&self, status: Option<&str>, limit: usize) -> Result<Vec<Proposal>> {
        let c = self.conn.lock().unwrap();
        let mut st = c
            .prepare(&format!("SELECT {COLUMNS} FROM proposals WHERE (?1 IS NULL OR status = ?1) ORDER BY seq DESC LIMIT ?2"))
            .map_err(db)?;
        let rows = st.query_map(params![status, limit as i64], row).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    /// Whether a proposal with this source key exists in any status (what keeps a tick from proposing the same thing twice).
    pub fn proposal_source_exists(&self, source: &str) -> Result<bool> {
        let n: i64 = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM proposals WHERE source=?1", params![source], |r| r.get(0))
            .map_err(db)?;
        Ok(n > 0)
    }

    /// Changes the status (and what goes with it) of an open or approved proposal, only from `from`.
    /// Returns whether a row changed (a second click on the same button changes nothing).
    pub fn proposal_transition(
        &self,
        id: &str,
        from: &[&str],
        to: &str,
        decided_at: Option<&str>,
        run: Option<(&str, &str)>,
        instruction: Option<&str>,
    ) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let marks = from.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "UPDATE proposals SET status=?, decided_at=COALESCE(?, decided_at), run_id=COALESCE(?, run_id), session_id=COALESCE(?, session_id), instruction=COALESCE(?, instruction) WHERE id=? AND status IN ({marks})"
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(to.to_string()),
            Box::new(decided_at.map(String::from)),
            Box::new(run.map(|(r, _)| r.to_string())),
            Box::new(run.map(|(_, s)| s.to_string())),
            Box::new(instruction.map(String::from)),
            Box::new(id.to_string()),
        ];
        args.extend(from.iter().map(|s| Box::new(s.to_string()) as Box<dyn rusqlite::ToSql>));
        let n = c.execute(&sql, rusqlite::params_from_iter(args.iter().map(|b| b.as_ref()))).map_err(db)?;
        Ok(n > 0)
    }

    /// Open proposals past `now` become `expired`. Returns how many.
    pub fn proposal_expire(&self, now: &str) -> Result<usize> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE proposals SET status='expired', decided_at=?1 WHERE status='open' AND expires_at <= ?1", params![now])
            .map_err(db)
    }

    /// `(finished runs, failed ones)` that started in `[from, to)` (RFC 3339 strings compare in order for one offset).
    pub fn run_counts_between(&self, from: &str, to: &str) -> Result<(u32, u32)> {
        let c = self.conn.lock().unwrap();
        let total: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM runs WHERE started_at >= ?1 AND started_at < ?2 AND status != 'running'",
                params![from, to],
                |r| r.get(0),
            )
            .map_err(db)?;
        let failed: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM runs WHERE started_at >= ?1 AND started_at < ?2 AND status = 'failed'",
                params![from, to],
                |r| r.get(0),
            )
            .map_err(db)?;
        Ok((total as u32, failed as u32))
    }
}
