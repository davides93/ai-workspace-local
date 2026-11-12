//! Storage of the API access tokens: the `api_tokens` table. Only a salted hash of each token is
//! kept (never the token), plus what the person needs to recognise and date it. The migration is
//! `IF NOT EXISTS` and runs on every start: a database from before this table simply gets it.

use super::{db, Store};
use aiw_core::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// One row of `api_tokens`.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub device_id: Option<String>,
    pub salt: String,
    pub hash: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub last_used_ip: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

const COLUMNS: &str =
    "id, name, kind, device_id, salt, hash, prefix, scopes, created_at, last_used_at, last_used_ip, expires_at, revoked_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<TokenRow> {
    let scopes: String = r.get(7)?;
    Ok(TokenRow {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        device_id: r.get(3)?,
        salt: r.get(4)?,
        hash: r.get(5)?,
        prefix: r.get(6)?,
        scopes: serde_json::from_str(&scopes).unwrap_or_default(),
        created_at: r.get(8)?,
        last_used_at: r.get(9)?,
        last_used_ip: r.get(10)?,
        expires_at: r.get(11)?,
        revoked_at: r.get(12)?,
    })
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_tokens (
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
         CREATE INDEX IF NOT EXISTS api_tokens_prefix ON api_tokens(prefix);
         CREATE INDEX IF NOT EXISTS api_tokens_device ON api_tokens(device_id);",
    )
    .map_err(db)
}

impl Store {
    pub fn token_insert(&self, t: &TokenRow) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                &format!("INSERT INTO api_tokens({COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
                params![
                    t.id,
                    t.name,
                    t.kind,
                    t.device_id,
                    t.salt,
                    t.hash,
                    t.prefix,
                    serde_json::to_string(&t.scopes)?,
                    t.created_at,
                    t.last_used_at,
                    t.last_used_ip,
                    t.expires_at,
                    t.revoked_at
                ],
            )
            .map_err(db)?;
        Ok(())
    }

    pub fn token_get(&self, id: &str) -> Result<Option<TokenRow>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(&format!("SELECT {COLUMNS} FROM api_tokens WHERE id=?1"), params![id], row)
            .optional()
            .map_err(db)
    }

    /// Every token, newest first (revoked ones included: the list shows them as such).
    pub fn token_list(&self) -> Result<Vec<TokenRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM api_tokens ORDER BY created_at DESC, rowid DESC")).map_err(db)?;
        let rows = st.query_map([], row).map_err(db)?.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
        Ok(rows)
    }

    /// The candidates for a presented token: the rows that share its visible prefix.
    pub fn token_by_prefix(&self, prefix: &str) -> Result<Vec<TokenRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM api_tokens WHERE prefix=?1")).map_err(db)?;
        let rows = st.query_map(params![prefix], row).map_err(db)?.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
        Ok(rows)
    }

    /// Marks a token revoked. `false` when it does not exist or already was.
    pub fn token_revoke(&self, id: &str, at: &str) -> Result<bool> {
        let n = self
            .conn
            .lock()
            .unwrap()
            .execute("UPDATE api_tokens SET revoked_at=?2 WHERE id=?1 AND revoked_at IS NULL", params![id, at])
            .map_err(db)?;
        Ok(n > 0)
    }

    pub fn token_set_expiry(&self, id: &str, expires_at: Option<&str>) -> Result<()> {
        self.conn.lock().unwrap().execute("UPDATE api_tokens SET expires_at=?2 WHERE id=?1", params![id, expires_at]).map_err(db)?;
        Ok(())
    }

    pub fn token_touch(&self, id: &str, at: &str, ip: Option<&str>) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE api_tokens SET last_used_at=?2, last_used_ip=?3 WHERE id=?1", params![id, at, ip])
            .map_err(db)?;
        Ok(())
    }

    /// Replaces the hash, salt and prefix of a token (used only for the owner token, which keeps its row).
    pub fn token_set_secret(&self, id: &str, salt: &str, hash: &str, prefix: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE api_tokens SET salt=?2, hash=?3, prefix=?4 WHERE id=?1", params![id, salt, hash, prefix])
            .map_err(db)?;
        Ok(())
    }

    /// Raw dump of the table, for the test that checks that no secret is at rest.
    pub fn token_dump(&self) -> String {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM api_tokens")).unwrap();
        let rows = st
            .query_map([], |r| (0..13).map(|i| r.get::<_, Option<String>>(i)).collect::<rusqlite::Result<Vec<_>>>())
            .unwrap()
            .map(|r| format!("{:?}", r.unwrap()))
            .collect::<Vec<_>>();
        rows.join("\n")
    }
}
