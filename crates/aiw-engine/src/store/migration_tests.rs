//! Schema migrations proved from every earlier stage of the database.
//!
//! The DDL below is frozen from the git history of `store.rs` and `store/*.rs` (`git log -p`), not taken from the
//! current code: a stage is what an installation of that time had on disk. Before the schema was stamped, every start
//! ran `CREATE TABLE IF NOT EXISTS` and the schema only grew by new tables (no column was ever added or changed: the
//! `CREATE TABLE` of every table that exists today is the one it was created with). Stages:
//!
//! 1. `a39285f`: documents, sessions, messages, events, runs, audit, workflow_runs.
//! 2. `955ae34`..`008b654`: workflow_state, feedback, eval_runs, a2a_tasks.
//! 3. `196d579`, `31b37b1`: train_jobs, profile_rewards.
//! 4. `4b70465`: memories (+ FTS5 and triggers), proposals.
//! 5. `ca52bad`: kb_collections, kb_documents, kb_chunks (+ FTS5 and triggers).
//! 6. `d78681a`: api_tokens.

use super::*;
use aiw_core::api::ChatMessage;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const STAGE_1: &str = "
CREATE TABLE documents (kind TEXT NOT NULL, id TEXT NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(kind, id));
CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL, agent_id TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_status TEXT);
CREATE TABLE messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, seq INTEGER NOT NULL, json TEXT NOT NULL);
CREATE INDEX messages_session ON messages(session_id, seq);
CREATE TABLE events (run_id TEXT NOT NULL, seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(run_id, seq));
CREATE TABLE runs (id TEXT PRIMARY KEY, session_id TEXT, status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT);
CREATE TABLE audit (id TEXT PRIMARY KEY, ts TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE workflow_runs (id TEXT PRIMARY KEY, workflow_id TEXT NOT NULL, started_at TEXT NOT NULL, json TEXT NOT NULL);
";

const STAGE_2: &str = "
CREATE TABLE workflow_state (run_id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE feedback (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, message_id TEXT NOT NULL, run_id TEXT, rating INTEGER NOT NULL, note TEXT, created_at TEXT NOT NULL);
CREATE UNIQUE INDEX feedback_message ON feedback(session_id, message_id);
CREATE TABLE eval_runs (id TEXT PRIMARY KEY, suite_id TEXT NOT NULL, started_at TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE a2a_tasks (id TEXT PRIMARY KEY, context_id TEXT NOT NULL, session_id TEXT, json TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE INDEX a2a_tasks_context ON a2a_tasks(context_id, updated_at);
";

const STAGE_3: &str = "
CREATE TABLE train_jobs (id TEXT PRIMARY KEY, kind TEXT NOT NULL, trainer TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, json TEXT NOT NULL);
CREATE TABLE profile_rewards (agent_id TEXT NOT NULL, key TEXT NOT NULL, profile_id TEXT NOT NULL, reward REAL NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(agent_id, key));
";

const STAGE_4: &str = "
CREATE TABLE memories (
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
CREATE INDEX memories_status ON memories(status, scope);
CREATE VIRTUAL TABLE memories_fts USING fts5(text, content='memories', content_rowid='seq', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
END;
CREATE TRIGGER memories_au AFTER UPDATE OF text ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, text) VALUES ('delete', old.seq, old.text);
  INSERT INTO memories_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TABLE proposals (
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
CREATE INDEX proposals_status ON proposals(status, created_at);
CREATE INDEX proposals_source ON proposals(source);
";

const STAGE_5: &str = "
CREATE TABLE kb_collections (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL);
CREATE TABLE kb_documents (
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
CREATE INDEX kb_documents_collection ON kb_documents(collection_id, created_at);
CREATE TABLE kb_chunks (
    rid INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    document_id TEXT NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    text TEXT NOT NULL,
    vector BLOB,
    embedding_model TEXT
);
CREATE INDEX kb_chunks_document ON kb_chunks(document_id, seq);
CREATE VIRTUAL TABLE kb_chunks_fts USING fts5(text, content='kb_chunks', content_rowid='rid', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
  INSERT INTO kb_chunks_fts(rowid, text) VALUES (new.rid, new.text);
END;
CREATE TRIGGER kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
  INSERT INTO kb_chunks_fts(kb_chunks_fts, rowid, text) VALUES ('delete', old.rid, old.text);
END;
";

const STAGE_6: &str = "
CREATE TABLE api_tokens (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    device_id TEXT,
    salt TEXT NOT NULL,
    hash TEXT NOT NULL,
    prefix TEXT NOT NULL,
    scopes TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    last_used_ip TEXT,
    expires_at TEXT,
    revoked_at TEXT
);
CREATE INDEX api_tokens_prefix ON api_tokens(prefix);
CREATE INDEX api_tokens_device ON api_tokens(device_id);
";

const STAGES: [&str; 6] = [STAGE_1, STAGE_2, STAGE_3, STAGE_4, STAGE_5, STAGE_6];

/// Data an installation of stage `stage` could have had, as the app of that time wrote it.
fn data_for(stage: usize) -> &'static str {
    match stage {
        1 => {
            "INSERT INTO documents VALUES ('agent', 'operator', '{\"id\":\"operator\",\"name\":\"Operatore\"}', '2026-11-02T10:00:00Z');
             INSERT INTO documents VALUES ('policy', 'default', '{\"rules\":[]}', '2026-11-02T10:00:01Z');
             INSERT INTO sessions VALUES ('ses_1', 'Riassunto del contratto', 'operator', '2026-11-02T10:00:00Z', '2026-11-02T10:05:00Z', 'completed');
             INSERT INTO runs VALUES ('run_1', 'ses_1', 'completed', '2026-11-02T10:00:10Z', '2026-11-02T10:00:40Z');
             INSERT INTO audit VALUES ('aud_1', '2026-11-02T10:00:11Z', '{\"id\":\"aud_1\",\"ts\":\"2026-11-02T10:00:11Z\",\"run_id\":\"run_1\",\"actor\":\"agent\",\"action\":\"fs.read\",\"detail\":\"README.md\"}');
             INSERT INTO workflow_runs VALUES ('wfr_1', 'wf_daily', '2026-11-02T11:00:00Z', '{\"id\":\"wfr_1\"}');"
        }
        2 => {
            "INSERT INTO feedback VALUES ('fb_1', 'ses_1', 'msg_2', 'run_1', 1, 'ottimo', '2026-11-05T09:00:00Z');
             INSERT INTO eval_runs VALUES ('evr_1', 'suite_a', '2026-11-05T09:10:00Z', '{\"passed\":3}');
             INSERT INTO a2a_tasks VALUES ('task_1', 'ctx_1', 'ses_1', '{\"id\":\"task_1\"}', '2026-11-05T09:20:00Z');
             INSERT INTO workflow_state VALUES ('wfr_1', '{\"step\":2}');"
        }
        3 => {
            "INSERT INTO train_jobs VALUES ('job_1', 'sft', 'mlx-lm', 'completed', '2026-11-06T08:00:00Z', '{\"id\":\"job_1\"}');
             INSERT INTO profile_rewards VALUES ('operator', 'thumb:msg_2', 'default', 1.0, '2026-11-06T08:30:00Z');"
        }
        4 => {
            "INSERT INTO memories (id, text, kind, scope, pinned, created_at, updated_at, status) VALUES ('mem_1', 'La caparra del contratto è di 2000 euro', 'fact', 'all', 1, '2026-11-12T08:00:00Z', '2026-11-12T08:00:00Z', 'active');
             INSERT INTO memories (id, text, kind, scope, pinned, created_at, updated_at, status) VALUES ('mem_2', 'Preferisco risposte brevi', 'preference', 'all', 0, '2026-11-12T08:01:00Z', '2026-11-12T08:01:00Z', 'active');
             INSERT INTO proposals (id, kind, title, noticed, instruction, source, agent_id, status, created_at, expires_at) VALUES ('prop_1', 'task', 'Rivedi le scadenze', 'Una scadenza è vicina', 'Controlla il calendario', 'calendar', 'operator', 'pending', '2026-11-12T08:05:00Z', '2026-11-13T08:05:00Z');"
        }
        5 => {
            "INSERT INTO kb_collections VALUES ('col_1', 'Contratti', 'Affitti e forniture', '2026-11-12T09:00:00Z');
             INSERT INTO kb_documents (id, collection_id, name, source, mime, size, sha256, status, chunk_count, created_at) VALUES ('doc_1', 'col_1', 'affitto.txt', 'file', 'text/plain', 120, 'abc', 'ready', 1, '2026-11-12T09:01:00Z');
             INSERT INTO kb_chunks (id, document_id, seq, text) VALUES ('chk_1', 'doc_1', 0, 'Il canone mensile è di 800 euro');
             INSERT INTO kb_documents (id, collection_id, name, source, mime, size, sha256, status, created_at) VALUES ('doc_2', 'col_1', 'in-corso.txt', 'file', 'text/plain', 10, 'def', 'indexing', '2026-11-12T09:02:00Z');"
        }
        6 => {
            "INSERT INTO api_tokens (id, name, kind, salt, hash, prefix, scopes, created_at) VALUES ('tok_1', 'Telefono', 'device', 's', 'h', 'aiw_ab', '[\"device\"]', '2026-11-12T10:00:00Z');"
        }
        _ => "",
    }
}

fn message(id: &str, role: &str, content: &str) -> String {
    serde_json::to_string(&ChatMessage {
        id: id.into(),
        role: role.into(),
        content: content.into(),
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        attachments: None,
        created_at: "2026-11-02T10:00:20Z".into(),
    })
    .unwrap()
}

/// A database file as the app of `stage` left it (`stamp` is the `user_version`, 0 before the stamp existed).
fn build(path: &Path, stage: usize, stamp: i64) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").unwrap();
    for ddl in &STAGES[..stage] {
        conn.execute_batch(ddl).unwrap();
    }
    for n in 1..=stage {
        conn.execute_batch(data_for(n)).unwrap();
    }
    let event =
        serde_json::to_string(&aiw_core::events::TimedEvent::now(aiw_core::events::AgEvent::StepStarted { step_name: "leggi".into() }))
            .unwrap();
    conn.execute("INSERT INTO events VALUES ('run_1', 0, ?1)", params![event]).unwrap();
    conn.execute("INSERT INTO messages VALUES ('msg_1', 'ses_1', 1, ?1)", params![message("msg_1", "user", "Riassumi l'affitto")]).unwrap();
    conn.execute(
        "INSERT INTO messages VALUES ('msg_2', 'ses_1', 2, ?1)",
        params![message("msg_2", "assistant", "Il canone è di 800 euro")],
    )
    .unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {stamp}")).unwrap();
}

fn schema_objects(conn: &Connection) -> BTreeSet<(String, String)> {
    let mut st = conn.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'").unwrap();
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

fn fresh_objects() -> BTreeSet<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    drop(Store::open(&path).unwrap());
    schema_objects(&Connection::open(&path).unwrap())
}

fn sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn dir_names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

#[test]
fn the_steps_are_in_order_and_end_at_the_current_version() {
    let versions: Vec<i64> = MIGRATIONS.iter().map(|m| m.0).collect();
    assert_eq!(versions, (1..=MIGRATIONS.len() as i64).collect::<Vec<_>>(), "versions are consecutive from 1");
    assert_eq!(*versions.last().unwrap(), SCHEMA_VERSION, "a new step must raise SCHEMA_VERSION");
    assert_eq!(STAGES.len() as i64, SCHEMA_VERSION, "a new step needs its stage here, with data and checks");
}

/// Every stage, with and without the stamp: the data survives, the schema ends equal to a new database's, the app can
/// write, and opening again changes nothing.
#[test]
fn every_earlier_stage_migrates_to_the_latest_and_keeps_its_data() {
    let fresh = fresh_objects();
    for stage in 1..=STAGES.len() {
        // 0: installations from before the stamp; `stage`: what a build with the stamp left at that version.
        for stamp in [0, stage as i64] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("aiw.db");
            build(&path, stage, stamp);
            let ctx = format!("stage {stage}, stamp {stamp}");

            let s = Store::open(&path).unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION, "{ctx}");

            // The data of every table the stage had is still there, readable through the store's own API.
            assert_eq!(s.list_docs::<serde_json::Value>("agent").unwrap()[0]["name"], "Operatore", "{ctx}");
            assert_eq!(s.sessions().unwrap()[0].title, "Riassunto del contratto", "{ctx}");
            let msgs = s.messages("ses_1").unwrap();
            assert_eq!(msgs.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["msg_1", "msg_2"], "{ctx}");
            assert_eq!(s.events("run_1").unwrap().len(), 1, "{ctx}");
            assert_eq!(s.audit(10).unwrap().len(), 1, "{ctx}");
            assert_eq!(s.session_runs("ses_1").unwrap()[0].status, "completed", "{ctx}");
            if stage >= 2 {
                assert_eq!(s.session_feedback("ses_1").unwrap()[0].note.as_deref(), Some("ottimo"), "{ctx}");
                assert!(s.a2a_get("task_1").unwrap().is_some(), "{ctx}");
            }
            if stage >= 3 {
                assert_eq!(s.profile_rewards("operator").unwrap(), vec![("default".to_string(), 1, 1.0)], "{ctx}");
            }
            if stage >= 4 {
                assert_eq!(
                    s.memory_fts("\"caparra\"*", &MemoryFilter::default(), 5).unwrap().unwrap(),
                    ["mem_1"],
                    "{ctx}: FTS5 index kept"
                );
            }
            if stage >= 5 {
                assert_eq!(s.kb_fts("\"canone\"*", &KbFilter::default(), 5).unwrap().unwrap(), ["chk_1"], "{ctx}");
                assert_eq!(s.kb_document("doc_2").unwrap().unwrap().status, "failed", "{ctx}: a document left indexing is failed at start");
            }
            if stage >= 6 {
                assert_eq!(s.token_list().unwrap()[0].name, "Telefono", "{ctx}");
            }

            // The tables the stage did not have exist now and work.
            s.put_doc("agent", "nuovo", &serde_json::json!({"x": 1})).unwrap();
            drop(s);

            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "INSERT INTO proposals (id, kind, title, noticed, instruction, source, agent_id, status, created_at, expires_at) VALUES ('zz', 'task', 't', 'n', 'i', 's', 'a', 'pending', 'x', 'y');
                 INSERT INTO kb_collections VALUES ('zz', 'Nuova', '', 'x');
                 INSERT INTO api_tokens (id, name, kind, salt, hash, prefix, scopes, created_at) VALUES ('zz', 'n', 'gateway', 's', 'h', 'p', '[]', 'x');
                 INSERT INTO memories (id, text, created_at, updated_at) VALUES ('zz', 'nuova', 'x', 'x');",
            )
            .unwrap_or_else(|e| panic!("{ctx}: the tables of the later stages work: {e}"));
            conn.execute_batch("DELETE FROM proposals WHERE id='zz'; DELETE FROM kb_collections WHERE id='zz'; DELETE FROM api_tokens WHERE id='zz'; DELETE FROM memories WHERE id='zz';").unwrap();
            assert_eq!(schema_objects(&conn), fresh, "{ctx}: the migrated schema is the one of a new database");
            let before: Vec<i64> = ["documents", "sessions", "messages", "audit", "runs"].iter().map(|t| count(&conn, t)).collect();
            drop(conn);

            // Idempotent: a second and a third start change nothing.
            for _ in 0..2 {
                let s = Store::open(&path).unwrap_or_else(|e| panic!("{ctx}: second open: {e}"));
                assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION, "{ctx}");
            }
            let conn = Connection::open(&path).unwrap();
            let after: Vec<i64> = ["documents", "sessions", "messages", "audit", "runs"].iter().map(|t| count(&conn, t)).collect();
            assert_eq!(before, after, "{ctx}");
            assert_eq!(schema_objects(&conn), fresh, "{ctx}");
        }
    }
}

#[test]
fn a_database_from_a_newer_build_is_refused_and_the_file_is_not_touched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aiw.db");
    build(&path, STAGES.len(), SCHEMA_VERSION + 3);
    // As the newer build left it: its own table the older one does not know.
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE future_things (id TEXT PRIMARY KEY); INSERT INTO future_things VALUES ('x');")
        .unwrap();
    let (hash, names) = (sha(&path), dir_names(dir.path()));

    let err = Store::open(&path).err().expect("a newer schema must be refused");
    let text = err.to_string();
    assert!(matches!(err, Error::Unsupported(_)), "{err:?}");
    assert!(
        text.contains(&format!("schema {}", SCHEMA_VERSION + 3)) && text.contains("più recente") && text.contains("non è stato modificato"),
        "{text}"
    );
    assert_eq!(err.status(), 422);

    assert_eq!(sha(&path), hash, "the database file is byte for byte the same");
    assert_eq!(dir_names(dir.path()), names, "no journal or other file was left behind");
    assert_eq!(schema_version_of(&path).unwrap(), SCHEMA_VERSION + 3);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(count(&conn, "future_things"), 1);
}

#[test]
fn a_step_that_fails_leaves_the_previous_version_and_none_of_its_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aiw.db");
    build(&path, 1, 1);
    // An object of the wrong kind with the name step 2 wants for an index.
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE a2a_tasks_context (x TEXT);").unwrap();
    let err = Store::open(&path).err().expect("the step cannot run").to_string();
    assert!(err.contains("versione 2") && err.contains("workflow durevoli"), "{err}");
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn).unwrap(), 1, "the stamp stays where the last good step left it");
    assert!(!schema_objects(&conn).contains(&("table".into(), "workflow_state".into())), "the failed step was rolled back whole");
    assert_eq!(count(&conn, "sessions"), 1, "the data is intact");
}

#[test]
fn an_empty_or_new_file_gets_the_whole_schema() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.db");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(Store::open(&empty).unwrap().schema_version().unwrap(), SCHEMA_VERSION);
    let missing = dir.path().join("missing.db");
    assert_eq!(Store::open(&missing).unwrap().schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(schema_objects(&Connection::open(&missing).unwrap()), fresh_objects());
}

#[test]
fn table_counts_list_the_data_tables_without_the_index_internals() {
    let s = Store::in_memory().unwrap();
    s.put_doc("agent", "a", &serde_json::json!({})).unwrap();
    let counts = s.table_counts().unwrap();
    let names: Vec<&str> = counts.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"documents") && names.contains(&"api_tokens") && names.contains(&"kb_chunks") && names.contains(&"memories"));
    assert!(!names.iter().any(|n| n.starts_with("sqlite_") || n.contains("_fts_")), "{names:?}");
    assert_eq!(counts.iter().find(|(n, _)| n == "documents").unwrap().1, 1);
}
