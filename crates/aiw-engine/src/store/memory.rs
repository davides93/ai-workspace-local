//! Storage of the long-term memory: the `memories` table, its FTS5 index (when the bundled SQLite
//! has it) and the optional embedding vectors. The ranking lives in [`crate::memory`].

use super::{db, Store};
use aiw_core::api::Memory;
use aiw_core::Result;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

const COLUMNS: &str =
    "id, text, kind, scope, pinned, session_id, created_at, updated_at, last_used_at, due_at, status, embedding IS NOT NULL";

/// Which memories a listing keeps. Every field narrows the result.
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    /// Any of these statuses; empty = every status.
    pub statuses: Vec<String>,
    pub kind: Option<String>,
    /// Only what this agent can see: memories for every assistant and those of this agent. `None`
    /// = no restriction (the person looking at their own memories).
    pub agent: Option<String>,
    pub pinned: Option<bool>,
    pub limit: Option<usize>,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        text: r.get(1)?,
        kind: r.get(2)?,
        scope: r.get(3)?,
        pinned: r.get::<_, i64>(4)? != 0,
        session_id: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        last_used_at: r.get(8)?,
        due_at: r.get(9)?,
        status: r.get(10)?,
        has_embedding: r.get::<_, i64>(11)? != 0,
    })
}

/// `(where clause, parameters)` for a filter; every value is a bound parameter.
fn where_of(f: &MemoryFilter, alias: &str) -> (String, Vec<String>) {
    let mut conds: Vec<String> = vec!["1=1".into()];
    let mut args: Vec<String> = vec![];
    if !f.statuses.is_empty() {
        let marks = vec!["?"; f.statuses.len()].join(",");
        conds.push(format!("{alias}status IN ({marks})"));
        args.extend(f.statuses.iter().cloned());
    }
    if let Some(k) = &f.kind {
        conds.push(format!("{alias}kind = ?"));
        args.push(k.clone());
    }
    if let Some(a) = &f.agent {
        conds.push(format!("({alias}scope = 'all' OR {alias}scope = ?)"));
        args.push(a.clone());
    }
    if let Some(p) = f.pinned {
        conds.push(format!("{alias}pinned = {}", if p { 1 } else { 0 }));
    }
    (conds.join(" AND "), args)
}

pub(crate) fn f32_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub(crate) fn blob_to_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Creates the table and, when this SQLite has FTS5, the index kept in step by triggers. Every
/// statement is `IF NOT EXISTS`: it runs on each start and leaves an existing database alone.
/// Returns whether the full-text index is available.
pub(super) fn migrate(conn: &Connection) -> Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            text TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'fact',
            scope TEXT NOT NULL DEFAULT 'all',
            pinned INTEGER NOT NULL DEFAULT 0,
            session_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_used_at TEXT,
            due_at TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            embedding BLOB,
            embedding_model TEXT
         );
         CREATE INDEX IF NOT EXISTS memories_status ON memories(status, scope);",
    )
    .map_err(db)?;
    let existed: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE name = 'memories_fts'", [], |_| Ok(true))
        .optional()
        .map_err(db)?
        .unwrap_or(false);
    let fts = conn
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(text, content='memories', content_rowid='seq', tokenize='unicode61 remove_diacritics 2');
             CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
               INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
             END;
             CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
               INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
             END;
             CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE OF text ON memories BEGIN
               INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
               INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
             END;",
        )
        .is_ok();
    if fts && !existed {
        // Rows that were there before the index existed.
        conn.execute("INSERT INTO memories_fts(memories_fts) VALUES ('rebuild')", []).map_err(db)?;
    }
    Ok(fts)
}

/// Is the full-text index there (it is created by [`migrate`] when this SQLite has FTS5)?
pub(super) fn has_fts(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM sqlite_master WHERE name = 'memories_fts'", [], |_| Ok(true)).optional().map_err(db)?.unwrap_or(false))
}

impl Store {
    /// Does the bundled SQLite have the FTS5 index? When not, [`crate::memory`] scores in Rust.
    pub fn memory_has_fts(&self) -> bool {
        self.fts
    }

    pub fn memory_insert(&self, m: &Memory) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memories (id, text, kind, scope, pinned, session_id, created_at, updated_at, last_used_at, due_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                m.id,
                m.text,
                m.kind,
                m.scope,
                m.pinned as i64,
                m.session_id,
                m.created_at,
                m.updated_at,
                m.last_used_at,
                m.due_at,
                m.status
            ],
        )
        .map_err(db)?;
        Ok(())
    }

    pub fn memory_get(&self, id: &str) -> Result<Option<Memory>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(&format!("SELECT {COLUMNS} FROM memories WHERE id = ?1"), [id], row).optional().map_err(db)
    }

    /// Pinned first, then the most recently changed.
    pub fn memory_list(&self, f: &MemoryFilter) -> Result<Vec<Memory>> {
        let (cond, args) = where_of(f, "");
        let limit = f.limit.map(|n| format!(" LIMIT {n}")).unwrap_or_default();
        let sql = format!("SELECT {COLUMNS} FROM memories WHERE {cond} ORDER BY pinned DESC, updated_at DESC, id{limit}");
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(args.iter()), row).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    /// Writes the changed fields of `m` back (not the embedding: a changed text drops it).
    pub fn memory_save(&self, m: &Memory) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "UPDATE memories SET
                   embedding = CASE WHEN text = ?2 THEN embedding ELSE NULL END,
                   embedding_model = CASE WHEN text = ?2 THEN embedding_model ELSE NULL END,
                   text = ?2, kind = ?3, scope = ?4, pinned = ?5, updated_at = ?6, last_used_at = ?7, due_at = ?8, status = ?9
                 WHERE id = ?1",
                params![m.id, m.text, m.kind, m.scope, m.pinned as i64, m.updated_at, m.last_used_at, m.due_at, m.status],
            )
            .map_err(db)?;
        Ok(n > 0)
    }

    /// Marks memories as used now (they show up first when pinned memories are cut to a budget).
    pub fn memory_touch(&self, ids: &[String], ts: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute("UPDATE memories SET last_used_at = ?2 WHERE id = ?1", params![id, ts]).map_err(db)?;
        }
        Ok(())
    }

    pub fn memory_delete(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM memories WHERE id = ?1", [id]).map_err(db)? > 0)
    }

    /// Hard-deletes every memory; returns how many.
    pub fn memory_delete_all(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM memories", []).map_err(db)
    }

    pub fn memory_set_embedding(&self, id: &str, model: &str, vector: &[f32]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE memories SET embedding = ?2, embedding_model = ?3 WHERE id = ?1", params![id, f32_to_blob(vector), model])
            .map_err(db)?;
        Ok(())
    }

    /// `(id, text)` of what still needs a vector for `model`: no vector, or one of another model.
    pub fn memory_missing_embeddings(&self, model: &str, limit: usize) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare(
                "SELECT id, text FROM memories WHERE status != 'forgotten' AND (embedding IS NULL OR embedding_model IS NOT ?1)
                 ORDER BY updated_at DESC, id LIMIT ?2",
            )
            .map_err(db)?;
        let rows = st.query_map(params![model, limit as i64], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    /// The vectors of `model` for the memories the filter keeps, in a stable order.
    pub fn memory_embeddings(&self, f: &MemoryFilter, model: &str) -> Result<Vec<(String, Vec<f32>)>> {
        let (cond, mut args) = where_of(f, "");
        args.push(model.to_string());
        let sql = format!("SELECT id, embedding FROM memories WHERE {cond} AND embedding IS NOT NULL AND embedding_model = ? ORDER BY id");
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(args.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))).map_err(db)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?.into_iter().map(|(id, b)| (id, blob_to_f32(&b))).collect())
    }

    /// Ids matching an FTS5 expression, best first (bm25). `None` when this SQLite has no FTS5.
    pub fn memory_fts(&self, expr: &str, f: &MemoryFilter, limit: usize) -> Result<Option<Vec<String>>> {
        if !self.fts {
            return Ok(None);
        }
        let (cond, mut args) = where_of(f, "m.");
        args.insert(0, expr.to_string());
        let sql = format!(
            "SELECT m.id FROM memories_fts JOIN memories m ON m.seq = memories_fts.rowid
             WHERE memories_fts MATCH ? AND {cond} ORDER BY bm25(memories_fts), m.id LIMIT {limit}"
        );
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(args.iter()), |r| r.get::<_, String>(0)).map_err(db)?;
        Ok(Some(rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(id: &str, text: &str) -> Memory {
        Memory {
            id: id.into(),
            text: text.into(),
            kind: "fact".into(),
            scope: "all".into(),
            pinned: false,
            session_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            last_used_at: None,
            due_at: None,
            status: "active".into(),
            has_embedding: false,
        }
    }

    #[test]
    fn the_bundled_sqlite_has_fts5() {
        // The brief's check: a plain fts5 table can be created and queried.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x); INSERT INTO t(x) VALUES ('ciao mondo');").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM t WHERE t MATCH 'mondo'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        assert!(Store::in_memory().unwrap().memory_has_fts());
    }

    #[test]
    fn migration_is_idempotent_and_indexes_rows_that_predate_the_index() {
        let conn = Connection::open_in_memory().unwrap();
        // A database from before the FTS index: the table exists with rows, the index does not.
        conn.execute_batch(
            "CREATE TABLE memories (seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, text TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'fact',
              scope TEXT NOT NULL DEFAULT 'all', pinned INTEGER NOT NULL DEFAULT 0, session_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
              last_used_at TEXT, due_at TEXT, status TEXT NOT NULL DEFAULT 'active', embedding BLOB, embedding_model TEXT);
             INSERT INTO memories (id, text, created_at, updated_at) VALUES ('mem_old', 'amo il caffè', 'x', 'x');",
        )
        .unwrap();
        assert!(migrate(&conn).unwrap());
        assert!(migrate(&conn).unwrap());
        let n: i64 = conn.query_row("SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'caffe'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "diacritics are folded and the old row is indexed");
    }

    #[test]
    fn crud_filter_and_index_follow_the_rows() {
        let s = Store::in_memory().unwrap();
        s.memory_insert(&mem("a", "mi chiamo Anna")).unwrap();
        let mut b = mem("b", "preferisco risposte brevi");
        b.scope = "operator".into();
        b.status = "proposed".into();
        s.memory_insert(&b).unwrap();
        assert_eq!(s.memory_list(&MemoryFilter::default()).unwrap().len(), 2);
        let f = MemoryFilter { agent: Some("other".into()), ..Default::default() };
        assert_eq!(s.memory_list(&f).unwrap().iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a"]);
        let f = MemoryFilter { statuses: vec!["proposed".into()], ..Default::default() };
        assert_eq!(s.memory_list(&f).unwrap()[0].id, "b");
        // FTS follows updates and deletes.
        let all = MemoryFilter::default();
        assert_eq!(s.memory_fts("\"anna\"", &all, 5).unwrap().unwrap(), ["a"]);
        let mut a = s.memory_get("a").unwrap().unwrap();
        a.text = "mi chiamo Marta".into();
        assert!(s.memory_save(&a).unwrap());
        assert!(s.memory_fts("\"anna\"", &all, 5).unwrap().unwrap().is_empty());
        assert_eq!(s.memory_fts("\"marta\"", &all, 5).unwrap().unwrap(), ["a"]);
        assert!(s.memory_delete("a").unwrap());
        assert!(s.memory_fts("\"marta\"", &all, 5).unwrap().unwrap().is_empty());
        assert_eq!(s.memory_delete_all().unwrap(), 1);
    }

    #[test]
    fn embeddings_roundtrip_and_a_changed_text_drops_them() {
        let s = Store::in_memory().unwrap();
        s.memory_insert(&mem("a", "uno")).unwrap();
        assert_eq!(s.memory_missing_embeddings("m1", 10).unwrap().len(), 1);
        s.memory_set_embedding("a", "m1", &[0.5, -1.0, 2.0]).unwrap();
        assert!(s.memory_missing_embeddings("m1", 10).unwrap().is_empty());
        assert_eq!(s.memory_missing_embeddings("m2", 10).unwrap().len(), 1, "another model needs its own vector");
        let got = s.memory_embeddings(&MemoryFilter::default(), "m1").unwrap();
        assert_eq!(got, vec![("a".to_string(), vec![0.5, -1.0, 2.0])]);
        assert!(s.memory_get("a").unwrap().unwrap().has_embedding);
        let mut a = s.memory_get("a").unwrap().unwrap();
        a.pinned = true;
        s.memory_save(&a).unwrap();
        assert!(s.memory_get("a").unwrap().unwrap().has_embedding, "same text keeps its vector");
        a.text = "due".into();
        s.memory_save(&a).unwrap();
        assert!(!s.memory_get("a").unwrap().unwrap().has_embedding);
    }
}
