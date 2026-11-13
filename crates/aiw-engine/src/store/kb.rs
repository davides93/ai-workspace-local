//! Storage of the knowledge base: collections, documents and their passages (chunks), the FTS5
//! index on the passages (when the bundled SQLite has it) and the optional embedding vectors.
//! Ranking and ingestion live in [`crate::kb`].
//!
//! Deleting a collection deletes its documents, deleting a document deletes its passages (foreign
//! keys with `ON DELETE CASCADE`; the triggers keep the FTS index in step).

use super::memory::{blob_to_f32, f32_to_blob};
use super::{db, Store};
use aiw_core::api::{KbCollection, KbDocument};
use aiw_core::Result;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

const DOC_COLUMNS: &str =
    "d.id, d.collection_id, d.name, d.source, d.mime, d.size, d.sha256, d.status, d.error, d.chunk_count, d.created_at,
     (SELECT count(*) FROM kb_chunks c WHERE c.document_id = d.id AND c.vector IS NOT NULL)";

/// A passage with the names needed to show it.
#[derive(Debug, Clone, PartialEq)]
pub struct KbChunkRow {
    pub id: String,
    pub document_id: String,
    pub document_name: String,
    pub collection_id: String,
    pub collection_name: String,
    pub seq: usize,
    pub text: String,
}

/// Which passages a search looks at: only those of documents that are ready, in these collections
/// (empty = every collection).
#[derive(Debug, Clone, Default)]
pub struct KbFilter {
    pub collections: Vec<String>,
}

/// `(where clause, parameters)`: every value is a bound parameter. Aliases: `c` chunks, `d` documents.
fn where_of(f: &KbFilter) -> (String, Vec<String>) {
    let mut cond = "d.status = 'ready'".to_string();
    let mut args = vec![];
    if !f.collections.is_empty() {
        cond.push_str(&format!(" AND d.collection_id IN ({})", vec!["?"; f.collections.len()].join(",")));
        args.extend(f.collections.iter().cloned());
    }
    (cond, args)
}

fn doc_row(r: &rusqlite::Row) -> rusqlite::Result<KbDocument> {
    Ok(KbDocument {
        id: r.get(0)?,
        collection_id: r.get(1)?,
        name: r.get(2)?,
        source: r.get(3)?,
        mime: r.get(4)?,
        size: r.get::<_, i64>(5)? as u64,
        sha256: r.get(6)?,
        status: r.get(7)?,
        error: r.get(8)?,
        chunks: r.get::<_, i64>(9)? as usize,
        created_at: r.get(10)?,
        embedded_chunks: r.get::<_, i64>(11)? as usize,
    })
}

fn chunk_row(r: &rusqlite::Row) -> rusqlite::Result<KbChunkRow> {
    Ok(KbChunkRow {
        id: r.get(0)?,
        document_id: r.get(1)?,
        document_name: r.get(2)?,
        collection_id: r.get(3)?,
        collection_name: r.get(4)?,
        seq: r.get::<_, i64>(5)? as usize,
        text: r.get(6)?,
    })
}

/// Creates the tables and, when this SQLite has FTS5, the index kept in step by triggers. Every
/// statement is `IF NOT EXISTS`: it runs on each start and leaves an existing database alone.
/// Documents left `indexing` by a previous run are marked failed (no job survives a restart).
/// Returns whether the full-text index is available.
pub(super) fn migrate(conn: &Connection) -> Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kb_collections (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS kb_documents (
            id TEXT PRIMARY KEY,
            collection_id TEXT NOT NULL REFERENCES kb_collections(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            source TEXT NOT NULL,
            mime TEXT NOT NULL,
            size INTEGER NOT NULL DEFAULT 0,
            sha256 TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'indexing',
            error TEXT,
            chunk_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS kb_documents_collection ON kb_documents(collection_id, created_at);
         CREATE TABLE IF NOT EXISTS kb_chunks (
            rid INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            document_id TEXT NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            text TEXT NOT NULL,
            vector BLOB,
            embedding_model TEXT
         );
         CREATE INDEX IF NOT EXISTS kb_chunks_document ON kb_chunks(document_id, seq);",
    )
    .map_err(db)?;
    conn.execute(
        "UPDATE kb_documents SET status = 'failed', error = 'Interrotto da un riavvio: aggiungilo di nuovo.' WHERE status = 'indexing'",
        [],
    )
    .map_err(db)?;
    let existed: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE name = 'kb_chunks_fts'", [], |_| Ok(true))
        .optional()
        .map_err(db)?
        .unwrap_or(false);
    let fts = conn
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS kb_chunks_fts USING fts5(text, content='kb_chunks', content_rowid='rid', tokenize='unicode61 remove_diacritics 2');
             CREATE TRIGGER IF NOT EXISTS kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
               INSERT INTO kb_chunks_fts(rowid, text) VALUES (new.rid, new.text);
             END;
             CREATE TRIGGER IF NOT EXISTS kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
               INSERT INTO kb_chunks_fts(kb_chunks_fts, rowid, text) VALUES ('delete', old.rid, old.text);
             END;",
        )
        .is_ok();
    if fts && !existed {
        conn.execute("INSERT INTO kb_chunks_fts(kb_chunks_fts) VALUES ('rebuild')", []).map_err(db)?;
    }
    Ok(fts)
}

/// What the knowledge base does at every start, whatever the schema version: documents left `indexing` by a previous
/// run are marked failed (no job survives a restart). Returns whether the full-text index is there.
pub(super) fn start(conn: &Connection) -> Result<bool> {
    conn.execute(
        "UPDATE kb_documents SET status = 'failed', error = 'Interrotto da un riavvio: aggiungilo di nuovo.' WHERE status = 'indexing'",
        [],
    )
    .map_err(db)?;
    Ok(conn
        .query_row("SELECT 1 FROM sqlite_master WHERE name = 'kb_chunks_fts'", [], |_| Ok(true))
        .optional()
        .map_err(db)?
        .unwrap_or(false))
}

impl Store {
    // ------------------------------------------------------------ collections
    pub fn kb_collection_insert(&self, c: &KbCollection) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO kb_collections (id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![c.id, c.name, c.description, c.created_at],
        )
        .map_err(db)?;
        Ok(())
    }

    fn collections_where(&self, cond: &str, args: &[&str]) -> Result<Vec<KbCollection>> {
        let sql = format!(
            "SELECT k.id, k.name, k.description, k.created_at,
                    (SELECT count(*) FROM kb_documents d WHERE d.collection_id = k.id),
                    (SELECT count(*) FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id WHERE d.collection_id = k.id AND d.status = 'ready'),
                    (SELECT count(*) FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id WHERE d.collection_id = k.id AND d.status = 'ready' AND c.vector IS NOT NULL)
             FROM kb_collections k WHERE {cond} ORDER BY k.created_at, k.id"
        );
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(KbCollection {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    description: r.get(2)?,
                    created_at: r.get(3)?,
                    documents: r.get::<_, i64>(4)? as usize,
                    chunks: r.get::<_, i64>(5)? as usize,
                    embedded_chunks: r.get::<_, i64>(6)? as usize,
                })
            })
            .map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    pub fn kb_collections(&self) -> Result<Vec<KbCollection>> {
        self.collections_where("1=1", &[])
    }

    pub fn kb_collection(&self, id: &str) -> Result<Option<KbCollection>> {
        Ok(self.collections_where("k.id = ?", &[id])?.into_iter().next())
    }

    /// A collection by name, ignoring case.
    pub fn kb_collection_by_name(&self, name: &str) -> Result<Option<KbCollection>> {
        Ok(self.collections_where("lower(k.name) = lower(?)", &[name])?.into_iter().next())
    }

    pub fn kb_collection_update(&self, id: &str, name: &str, description: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .execute("UPDATE kb_collections SET name = ?2, description = ?3 WHERE id = ?1", params![id, name, description])
            .map_err(db)?
            > 0)
    }

    pub fn kb_collection_delete(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM kb_collections WHERE id = ?1", [id]).map_err(db)? > 0)
    }

    // ------------------------------------------------------------ documents
    pub fn kb_document_insert(&self, d: &KbDocument) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO kb_documents (id, collection_id, name, source, mime, size, sha256, status, error, chunk_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                d.id,
                d.collection_id,
                d.name,
                d.source,
                d.mime,
                d.size as i64,
                d.sha256,
                d.status,
                d.error,
                d.chunks as i64,
                d.created_at
            ],
        )
        .map_err(db)?;
        Ok(())
    }

    fn documents_where(&self, cond: &str, args: &[&str]) -> Result<Vec<KbDocument>> {
        let sql = format!("SELECT {DOC_COLUMNS} FROM kb_documents d WHERE {cond} ORDER BY d.created_at, d.id");
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(args.iter()), doc_row).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    pub fn kb_document(&self, id: &str) -> Result<Option<KbDocument>> {
        Ok(self.documents_where("d.id = ?", &[id])?.into_iter().next())
    }

    pub fn kb_documents(&self, collection_id: &str) -> Result<Vec<KbDocument>> {
        self.documents_where("d.collection_id = ?", &[collection_id])
    }

    /// The document of a collection with this content, if any.
    pub fn kb_document_by_hash(&self, collection_id: &str, sha256: &str) -> Result<Option<KbDocument>> {
        Ok(self.documents_where("d.collection_id = ? AND d.sha256 = ?", &[collection_id, sha256])?.into_iter().next())
    }

    /// The document of a collection that came from this path, if any.
    pub fn kb_document_by_source(&self, collection_id: &str, source: &str) -> Result<Option<KbDocument>> {
        Ok(self.documents_where("d.collection_id = ? AND d.source = ?", &[collection_id, source])?.into_iter().next())
    }

    /// Stores the passages of a document and marks it ready, in one transaction.
    pub fn kb_document_ready(&self, document_id: &str, chunks: &[(String, String)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(db)?;
        {
            let mut ins = tx.prepare("INSERT INTO kb_chunks (id, document_id, seq, text) VALUES (?1, ?2, ?3, ?4)").map_err(db)?;
            for (i, (id, text)) in chunks.iter().enumerate() {
                ins.execute(params![id, document_id, (i + 1) as i64, text]).map_err(db)?;
            }
        }
        tx.execute(
            "UPDATE kb_documents SET status = 'ready', error = NULL, chunk_count = ?2 WHERE id = ?1",
            params![document_id, chunks.len() as i64],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }

    pub fn kb_document_failed(&self, document_id: &str, error: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE kb_documents SET status = 'failed', error = ?2 WHERE id = ?1", params![document_id, error]).map_err(db)?;
        Ok(())
    }

    pub fn kb_document_delete(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM kb_documents WHERE id = ?1", [id]).map_err(db)? > 0)
    }

    // ------------------------------------------------------------ passages
    /// The passages with these ids, with the names of their document and collection.
    pub fn kb_chunks(&self, ids: &[String]) -> Result<Vec<KbChunkRow>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT c.id, d.id, d.name, k.id, k.name, c.seq, c.text FROM kb_chunks c
             JOIN kb_documents d ON d.id = c.document_id JOIN kb_collections k ON k.id = d.collection_id
             WHERE c.id IN ({})",
            vec!["?"; ids.len()].join(",")
        );
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(ids.iter()), chunk_row).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    /// Ids of the passages matching an FTS5 expression, best first (bm25). `None` when this
    /// SQLite has no FTS5.
    pub fn kb_fts(&self, expr: &str, f: &KbFilter, limit: usize) -> Result<Option<Vec<String>>> {
        if !self.kb_fts {
            return Ok(None);
        }
        let (cond, mut args) = where_of(f);
        args.insert(0, expr.to_string());
        let sql = format!(
            "SELECT c.id FROM kb_chunks_fts JOIN kb_chunks c ON c.rid = kb_chunks_fts.rowid JOIN kb_documents d ON d.id = c.document_id
             WHERE kb_chunks_fts MATCH ? AND {cond} ORDER BY bm25(kb_chunks_fts), c.id LIMIT {limit}"
        );
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let rows = st.query_map(params_from_iter(args.iter()), |r| r.get::<_, String>(0)).map_err(db)?;
        Ok(Some(rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?))
    }

    /// Calls `f(id, text)` for every searchable passage, in a stable order, without holding them
    /// all in memory (the keyword search when this SQLite has no FTS5).
    pub fn kb_scan_texts(&self, f: &KbFilter, mut each: impl FnMut(&str, &str)) -> Result<()> {
        let (cond, args) = where_of(f);
        let sql = format!("SELECT c.id, c.text FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id WHERE {cond} ORDER BY c.rid");
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let mut rows = st.query(params_from_iter(args.iter())).map_err(db)?;
        while let Some(r) = rows.next().map_err(db)? {
            each(&r.get::<_, String>(0).map_err(db)?, &r.get::<_, String>(1).map_err(db)?);
        }
        Ok(())
    }

    /// Calls `f(id, vector)` for every searchable passage with a vector of `model`, in a stable
    /// order, one row at a time: a large collection is never loaded whole.
    pub fn kb_scan_vectors(&self, f: &KbFilter, model: &str, mut each: impl FnMut(&str, &[f32])) -> Result<()> {
        let (cond, mut args) = where_of(f);
        args.push(model.to_string());
        let sql = format!(
            "SELECT c.id, c.vector FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id
             WHERE {cond} AND c.vector IS NOT NULL AND c.embedding_model = ? ORDER BY c.rid"
        );
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(&sql).map_err(db)?;
        let mut rows = st.query(params_from_iter(args.iter())).map_err(db)?;
        while let Some(r) = rows.next().map_err(db)? {
            let id: String = r.get(0).map_err(db)?;
            let blob: Vec<u8> = r.get(1).map_err(db)?;
            each(&id, &blob_to_f32(&blob));
        }
        Ok(())
    }

    /// Is there at least one searchable passage with a vector of `model`?
    pub fn kb_has_vectors(&self, model: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id
             WHERE d.status = 'ready' AND c.vector IS NOT NULL AND c.embedding_model = ?1 LIMIT 1",
            [model],
            |_| Ok(true),
        )
        .optional()
        .map_err(db)
        .map(|v| v.unwrap_or(false))
    }

    pub fn kb_set_embedding(&self, id: &str, model: &str, vector: &[f32]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE kb_chunks SET vector = ?2, embedding_model = ?3 WHERE id = ?1", params![id, f32_to_blob(vector), model])
            .map_err(db)?;
        Ok(())
    }

    /// `(id, text)` of the passages of ready documents that still need a vector for `model`: no
    /// vector, or one of another model.
    pub fn kb_missing_embeddings(&self, model: &str, limit: usize) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare(
                "SELECT c.id, c.text FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id
                 WHERE d.status = 'ready' AND (c.vector IS NULL OR c.embedding_model IS NOT ?1) ORDER BY c.rid LIMIT ?2",
            )
            .map_err(db)?;
        let rows = st.query_map(params![model, limit as i64], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
    }

    /// Forgets the vectors of a collection (they are computed again by the next embedding pass).
    pub fn kb_clear_embeddings(&self, collection_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE kb_chunks SET vector = NULL, embedding_model = NULL
             WHERE vector IS NOT NULL AND document_id IN (SELECT id FROM kb_documents WHERE collection_id = ?1)",
            [collection_id],
        )
        .map_err(db)
    }

    /// Passages in the whole base (all statuses): tests and the cascade checks.
    pub fn kb_chunk_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT count(*) FROM kb_chunks", [], |r| r.get::<_, i64>(0)).map(|n| n as usize).map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coll(id: &str, name: &str) -> KbCollection {
        KbCollection {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            documents: 0,
            chunks: 0,
            embedded_chunks: 0,
        }
    }

    fn doc(id: &str, coll: &str, name: &str) -> KbDocument {
        KbDocument {
            id: id.into(),
            collection_id: coll.into(),
            name: name.into(),
            source: "caricato".into(),
            mime: "text/plain".into(),
            size: 3,
            sha256: format!("h-{id}"),
            status: "indexing".into(),
            error: None,
            chunks: 0,
            embedded_chunks: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn ch(id: &str, text: &str) -> (String, String) {
        (id.into(), text.into())
    }

    #[test]
    fn migration_is_idempotent_and_fails_documents_left_indexing() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        assert!(migrate(&conn).unwrap());
        conn.execute("INSERT INTO kb_collections VALUES ('c', 'C', '', 'x')", []).unwrap();
        conn.execute("INSERT INTO kb_documents (id, collection_id, name, source, mime, sha256, created_at) VALUES ('d', 'c', 'n', 's', 'm', 'h', 'x')", [])
            .unwrap();
        assert!(migrate(&conn).unwrap());
        let status: String = conn.query_row("SELECT status FROM kb_documents WHERE id = 'd'", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "failed", "no job survives a restart");
    }

    #[test]
    fn deleting_a_document_or_a_collection_deletes_its_passages_and_index_entries() {
        let s = Store::in_memory().unwrap();
        s.kb_collection_insert(&coll("c1", "Contratti")).unwrap();
        s.kb_collection_insert(&coll("c2", "Ricette")).unwrap();
        for (d, c, text) in [("d1", "c1", "il canone mensile"), ("d2", "c1", "la caparra"), ("d3", "c2", "la pasta al forno")] {
            s.kb_document_insert(&doc(d, c, d)).unwrap();
            s.kb_document_ready(d, &[ch(&format!("{d}-1"), text)]).unwrap();
        }
        let all = KbFilter::default();
        assert_eq!(s.kb_chunk_count().unwrap(), 3);
        assert_eq!(s.kb_fts("\"canone\"*", &all, 5).unwrap().unwrap(), ["d1-1"]);
        // A document goes: its passage and its index entry go with it.
        assert!(s.kb_document_delete("d1").unwrap());
        assert!(s.kb_fts("\"canone\"*", &all, 5).unwrap().unwrap().is_empty());
        assert_eq!(s.kb_chunk_count().unwrap(), 2);
        // A collection goes: documents, passages, index.
        assert!(s.kb_collection_delete("c1").unwrap());
        assert!(s.kb_document("d2").unwrap().is_none());
        assert!(s.kb_fts("\"caparra\"*", &all, 5).unwrap().unwrap().is_empty());
        assert_eq!(s.kb_chunk_count().unwrap(), 1);
        assert_eq!(s.kb_fts("\"pasta\"*", &all, 5).unwrap().unwrap(), ["d3-1"]);
        assert!(!s.kb_collection_delete("c1").unwrap());
    }

    #[test]
    fn counts_filters_and_embeddings_follow_the_rows() {
        let s = Store::in_memory().unwrap();
        s.kb_collection_insert(&coll("c1", "Uno")).unwrap();
        s.kb_collection_insert(&coll("c2", "Due")).unwrap();
        s.kb_document_insert(&doc("d1", "c1", "a.txt")).unwrap();
        s.kb_document_insert(&doc("d2", "c2", "b.txt")).unwrap();
        // A document still indexing has no passages in search.
        s.kb_document_ready("d1", &[ch("p1", "alfa"), ch("p2", "beta")]).unwrap();
        let c = s.kb_collection("c1").unwrap().unwrap();
        assert_eq!((c.documents, c.chunks, c.embedded_chunks), (1, 2, 0));
        assert_eq!(s.kb_collection_by_name("UNO").unwrap().unwrap().id, "c1", "names ignore case");
        assert!(s.kb_fts("\"alfa\"*", &KbFilter { collections: vec!["c2".into()] }, 5).unwrap().unwrap().is_empty());
        assert_eq!(s.kb_documents("c1").unwrap()[0].chunks, 2);
        // Embeddings.
        assert_eq!(s.kb_missing_embeddings("m1", 10).unwrap().len(), 2);
        s.kb_set_embedding("p1", "m1", &[1.0, 0.5]).unwrap();
        assert_eq!(s.kb_missing_embeddings("m1", 10).unwrap().len(), 1);
        assert_eq!(s.kb_missing_embeddings("m2", 10).unwrap().len(), 2, "another model needs its own vectors");
        assert!(s.kb_has_vectors("m1").unwrap() && !s.kb_has_vectors("m2").unwrap());
        let mut seen = vec![];
        s.kb_scan_vectors(&KbFilter::default(), "m1", |id, v| seen.push((id.to_string(), v.to_vec()))).unwrap();
        assert_eq!(seen, vec![("p1".to_string(), vec![1.0, 0.5])]);
        assert_eq!(s.kb_documents("c1").unwrap()[0].embedded_chunks, 1);
        assert_eq!(s.kb_clear_embeddings("c1").unwrap(), 1);
        assert!(!s.kb_has_vectors("m1").unwrap());
        // Failure keeps the error.
        s.kb_document_failed("d2", "illeggibile").unwrap();
        let d2 = s.kb_document("d2").unwrap().unwrap();
        assert_eq!((d2.status.as_str(), d2.error.as_deref()), ("failed", Some("illeggibile")));
    }

    #[test]
    fn without_fts5_the_index_is_absent_and_the_passages_can_still_be_scanned() {
        let mut s = Store::in_memory().unwrap();
        s.kb_collection_insert(&coll("c1", "Uno")).unwrap();
        s.kb_document_insert(&doc("d1", "c1", "a.txt")).unwrap();
        s.kb_document_ready("d1", &[ch("p1", "il canone"), ch("p2", "la caparra")]).unwrap();
        s.kb_fts = false;
        assert_eq!(s.kb_fts("\"canone\"*", &KbFilter::default(), 5).unwrap(), None, "the engine falls back to a Rust score");
        let mut seen = vec![];
        s.kb_scan_texts(&KbFilter::default(), |id, t| seen.push(format!("{id}:{t}"))).unwrap();
        assert_eq!(seen, ["p1:il canone", "p2:la caparra"]);
        // Passages of a document that is not ready are not scanned.
        s.kb_document_insert(&doc("d2", "c1", "b.txt")).unwrap();
        let mut n = 0;
        s.kb_scan_texts(&KbFilter::default(), |_, _| n += 1).unwrap();
        assert_eq!(n, 2);
    }
}
