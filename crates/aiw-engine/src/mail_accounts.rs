//! The IMAP accounts of the mail connector. The list (host, port, user name, folders) lives in the
//! `connectors.mail.imap` setting; the password never does: it is kept in the store next to the other
//! secrets, shown by the API as [`SECRET_MASK`], and sent only to the server of its own account.

use crate::Engine;
use aiw_core::api::{MailAccount, MailAccountTest, MailFolderCount, SECRET_MASK};
use aiw_core::config::Layer;
use aiw_core::{Error, Result};
use aiw_tools::connectors::imap;
use serde_json::{json, Value};

const MAX_ACCOUNTS: usize = 5;

fn secret_id(id: &str) -> String {
    format!("imap:{id}")
}

fn slug(s: &str) -> String {
    let t: String =
        s.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    t.trim_matches('-').chars().take(32).collect()
}

impl Engine {
    /// The accounts as stored, without passwords.
    fn imap_stored(&self) -> Vec<MailAccount> {
        self.0.config.get("connectors.mail.imap").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    fn imap_password(&self, id: &str) -> Option<String> {
        self.0
            .store
            .get_doc::<Value>("secret", &secret_id(id))
            .ok()
            .flatten()
            .and_then(|v| v["value"].as_str().map(String::from))
            .filter(|p| !p.is_empty())
    }

    /// The accounts as the API shows them: the password is the mask when one is stored, empty otherwise.
    pub fn mail_accounts(&self) -> Vec<MailAccount> {
        self.imap_stored()
            .into_iter()
            .map(|mut a| {
                a.password = if self.imap_password(&a.id).is_some() { SECRET_MASK.into() } else { String::new() };
                a
            })
            .collect()
    }

    /// Replaces the list of accounts. A password sent as the mask keeps the stored one; an account that is
    /// no longer in the list loses its password.
    pub fn set_mail_accounts(&self, list: Vec<MailAccount>) -> Result<Vec<MailAccount>> {
        if list.len() > MAX_ACCOUNTS {
            return Err(Error::Invalid(format!("al massimo {MAX_ACCOUNTS} account")));
        }
        let mut clean: Vec<MailAccount> = Vec::new();
        let mut passwords: Vec<(String, Option<String>)> = Vec::new();
        for mut a in list {
            a.host = a.host.trim().to_string();
            a.username = a.username.trim().to_string();
            if a.host.is_empty() || a.host.contains(['/', ' ', ':']) {
                return Err(Error::Invalid(
                    "l'indirizzo del server IMAP non è valido (per esempio imap.example.org, senza http:// e senza porta)".into(),
                ));
            }
            if a.username.is_empty() {
                return Err(Error::Invalid("serve il nome utente dell'account".into()));
            }
            a.id = Some(slug(&a.id)).filter(|s| !s.is_empty()).unwrap_or_else(|| slug(&format!("{}-{}", a.username, a.host)));
            if clean.iter().any(|c| c.id == a.id) {
                return Err(Error::Invalid(format!("due account hanno lo stesso nome «{}»", a.id)));
            }
            if a.port == 0 {
                a.port = imap::DEFAULT_PORT;
            }
            a.folders = a.folders.iter().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
            if a.folders.is_empty() {
                a.folders = vec!["INBOX".into()];
            }
            let pw = std::mem::take(&mut a.password);
            passwords.push((a.id.clone(), if pw == SECRET_MASK { None } else { Some(pw) }));
            clean.push(a);
        }
        for (id, pw) in &passwords {
            match pw {
                None => {}
                Some(p) if p.is_empty() => {
                    self.0.store.delete_doc("secret", &secret_id(id))?;
                }
                Some(p) => self.0.store.put_doc("secret", &secret_id(id), &json!({"value": p}))?,
            }
        }
        for old in self.imap_stored() {
            if !clean.iter().any(|c| c.id == old.id) {
                self.0.store.delete_doc("secret", &secret_id(&old.id))?;
            }
        }
        self.0.config.set(Layer::Global, "connectors.mail.imap", json!(clean))?;
        self.sync_connectors();
        self.audit_user("connectors.mail.imap", &format!("{} account", clean.len()));
        Ok(self.mail_accounts())
    }

    /// The accounts with their passwords, for the connector.
    pub(crate) fn imap_runtime(&self) -> Vec<imap::Account> {
        self.imap_stored()
            .into_iter()
            .map(|a| imap::Account {
                password: self.imap_password(&a.id).unwrap_or_default(),
                id: a.id,
                host: a.host,
                port: a.port,
                username: a.username,
                folders: a.folders,
                tls: String::new(),
            })
            .collect()
    }

    /// The «Prova» button: connects, logs in and counts the recent messages of each folder. Nothing is read or changed.
    pub async fn mail_account_test(&self, a: MailAccount) -> MailAccountTest {
        let password = if a.password == SECRET_MASK { self.imap_password(&slug(&a.id)).unwrap_or_default() } else { a.password.clone() };
        if a.host.trim().is_empty() || a.username.trim().is_empty() {
            return MailAccountTest { ok: false, detail: "Compila il server e il nome utente.".into(), folders: vec![] };
        }
        let acct = imap::Account {
            id: a.id.clone(),
            host: a.host.trim().to_string(),
            port: a.port,
            username: a.username.trim().to_string(),
            password,
            folders: a.folders.clone(),
            tls: String::new(),
        };
        match aiw_tools::connectors::test_account(&acct, &self.tls_roots()).await {
            Ok(t) => {
                let total: usize = t.folders.iter().map(|f| f.1).sum();
                MailAccountTest {
                    ok: true,
                    detail: format!("Collegato: {total} messaggi negli ultimi 7 giorni. Non ho letto né cambiato nulla."),
                    folders: t.folders.into_iter().map(|(name, n)| MailFolderCount { name, recent: n as u32 }).collect(),
                }
            }
            Err(e) => MailAccountTest { ok: false, detail: e.to_string(), folders: vec![] },
        }
    }
}
