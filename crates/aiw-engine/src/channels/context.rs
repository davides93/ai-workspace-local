//! Shared contexts: one conversation that goes on across channels.
//!
//! By default every external conversation (a Telegram chat, a Slack direct message, a mail thread) has a session of
//! its own and nothing of one leaks into another. A **shared context** is the owner's explicit statement that two or
//! more accounts are the same person: they name each account (a sender on the list of a channel), and from then on a
//! direct message from any of them is answered in one shared session, with one history. Nothing here ever decides
//! that two accounts belong together, and linking never merges the sessions that already exist.
//!
//! The rules that keep it from becoming a way to read someone else's history:
//!
//! * **Only the owner links** (`/api/contexts` needs full access), and only an account that is on the list of its
//!   channel, once per channel, and in one context at most.
//! * **Only direct conversations.** A message from a group or a public channel is never routed here, because the answer
//!   would go to everyone who can read it. The connector says whether a message is direct; the default is «no».
//! * **Telegram: the user id**, never a chat id (a group is a negative one). **Email: only when the channel checks
//!   the sender** (`require_auth`), at link time and at every message, because a forged `From` would otherwise read
//!   the history of the other channels.
//! * **Never freer than the most restricted channel**, recomputed at every turn from what the channels say now: one
//!   read-only channel makes the whole context read-only. All channels of a context use the same assistant.
//! * **Leaving is automatic**: deleting a channel or taking an account off its list removes the membership; the
//!   session stays while other members remain, and it is a plain session of the app after the context is deleted.
//!
//! Confirmations are asked, and can be answered, only where the run that opened them was started (see `TurnOrigin` in
//! the parent module): a «sì» on another channel of the same context is told to answer where it was asked.

use super::*;

const MAX_MEMBERS: usize = 8;
const MAX_CONTEXTS: usize = 20;

fn auth_checked(ch: &Channel) -> bool {
    ch.settings.get("require_auth").and_then(|v| v.as_bool()).unwrap_or(true)
}

impl Engine {
    pub(super) fn contexts_raw(&self) -> Vec<ChannelContext> {
        let mut v = self.0.store.list_docs::<ChannelContext>("channel_context").unwrap_or_default();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        v
    }

    fn context_doc(&self, id: &str) -> Result<ChannelContext> {
        self.0
            .store
            .get_doc::<ChannelContext>("channel_context", id)?
            .ok_or_else(|| Error::NotFound(format!("contesto condiviso {id} non trovato")))
    }

    /// Why this account cannot take part right now (`None` when it can).
    fn member_problem(&self, m: &ContextMember) -> Option<String> {
        let Ok(ch) = self.channel_doc(&m.channel_id) else { return Some("il canale non esiste più".into()) };
        let sender = norm(&ch.kind, &m.sender);
        if !ch.allowed.contains(&sender) {
            return Some("non è più nella lista di chi può parlare con il canale".into());
        }
        match ch.kind.as_str() {
            "telegram" if sender.parse::<i64>().map(|n| n <= 0).unwrap_or(true) => {
                Some("serve l'identificativo di una persona: quello di un gruppo non si collega".into())
            }
            "email" if !auth_checked(&ch) => Some(
                "il controllo del mittente è spento su questo canale: un mittente falsificato leggerebbe la cronologia degli altri canali"
                    .into(),
            ),
            _ => None,
        }
    }

    /// The assistant every channel of the context uses, or `None` when they do not agree (or none is left).
    fn context_agent(&self, ctx: &ChannelContext) -> Option<String> {
        let mut agents = ctx.members.iter().filter_map(|m| self.channel_doc(&m.channel_id).ok()).map(|c| c.agent_id);
        let first = agents.next()?;
        agents.all(|a| a == first).then_some(first)
    }

    /// What the context may do: read-only as soon as one of its channels is, never more than «ask» otherwise.
    /// Computed from the channels as they are now.
    pub(super) fn context_kind(&self, ctx: &ChannelContext) -> RunKind {
        let channels: Vec<Channel> = ctx.members.iter().filter_map(|m| self.channel_doc(&m.channel_id).ok()).collect();
        if channels.is_empty() || channels.iter().any(|c| c.mode != MODE_ASK) {
            RunKind::ReadOnly
        } else {
            RunKind::Ask
        }
    }

    fn context_info(&self, ctx: &ChannelContext) -> ContextInfo {
        let members_info = ctx
            .members
            .iter()
            .map(|m| {
                let ch = self.channel_doc(&m.channel_id).ok();
                ContextMemberInfo {
                    channel_id: m.channel_id.clone(),
                    channel_name: ch.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
                    kind: ch.as_ref().map(|c| c.kind.clone()).unwrap_or_default(),
                    sender: m.sender.clone(),
                    problem: self.member_problem(m),
                }
            })
            .collect();
        let mut shown = ctx.clone();
        if let Some(a) = self.context_agent(ctx) {
            shown.agent_id = a;
        }
        ContextInfo {
            mode: if self.context_kind(ctx) == RunKind::Ask { MODE_ASK } else { MODE_READ_ONLY }.into(),
            session_title: self.0.store.session(&ctx.session_id).map(|s| s.title).unwrap_or_default(),
            context: shown,
            members_info,
        }
    }

    pub fn contexts(&self) -> Result<Vec<ContextInfo>> {
        Ok(self.contexts_raw().iter().map(|c| self.context_info(c)).collect())
    }

    pub fn context(&self, id: &str) -> Result<ContextInfo> {
        Ok(self.context_info(&self.context_doc(id)?))
    }

    /// Checks the accounts of a context (a new one, or the new list of an existing one): each is on the list of its
    /// channel, once per channel, free of any other context, all with the same assistant. Returns them normalised.
    fn context_check(&self, members: Vec<ContextMember>, except: Option<&str>) -> Result<Vec<ContextMember>> {
        if members.is_empty() {
            return Err(Error::Invalid("un contesto condiviso ha bisogno di almeno un account (canale e mittente)".into()));
        }
        if members.len() > MAX_MEMBERS {
            return Err(Error::Invalid(format!("al massimo {MAX_MEMBERS} account per contesto")));
        }
        let others = self.contexts_raw();
        let mut clean: Vec<ContextMember> = Vec::new();
        let mut agents: Vec<(String, String)> = Vec::new();
        for m in members {
            let ch =
                self.channel_doc(m.channel_id.trim()).map_err(|_| Error::Invalid(format!("il canale «{}» non esiste", m.channel_id)))?;
            let sender = norm(&ch.kind, &m.sender);
            let member = ContextMember { channel_id: ch.id.clone(), sender: sender.clone() };
            if clean.iter().any(|c| c.channel_id == ch.id) {
                return Err(Error::Invalid(format!(
                    "il canale «{}» compare due volte: in un contesto c'è un solo account per canale",
                    ch.name
                )));
            }
            if !ch.allowed.contains(&sender) {
                return Err(Error::Invalid(format!(
                    "«{sender}» non è nella lista del canale «{}»: aggiungilo prima a chi può parlare con l'assistente",
                    ch.name
                )));
            }
            if let Some(problem) = self.member_problem(&member) {
                return Err(Error::Invalid(format!("«{sender}» su «{}»: {problem}", ch.name)));
            }
            if let Some(other) = others
                .iter()
                .filter(|c| Some(c.id.as_str()) != except)
                .find(|c| c.members.iter().any(|x| x.channel_id == ch.id && x.sender == sender))
            {
                return Err(Error::Conflict(format!("«{sender}» su «{}» fa già parte del contesto «{}»", ch.name, other.name)));
            }
            agents.push((ch.name.clone(), ch.agent_id.clone()));
            clean.push(member);
        }
        if let Some((name, agent)) = agents.iter().find(|(_, a)| *a != agents[0].1) {
            return Err(Error::Invalid(format!(
                "i canali usano assistenti diversi («{}» usa «{}», «{name}» usa «{agent}»): imposta lo stesso assistente su tutti, altrimenti le risposte cambierebbero da un canale all'altro",
                agents[0].0, agents[0].1
            )));
        }
        Ok(clean)
    }

    fn context_name(raw: &str) -> Result<String> {
        let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err(Error::Invalid("il contesto ha bisogno di un nome".into()));
        }
        if name.chars().count() > 60 {
            return Err(Error::Invalid("il nome del contesto è troppo lungo (al massimo 60 caratteri)".into()));
        }
        Ok(name)
    }

    fn context_label(&self, members: &[ContextMember]) -> String {
        members
            .iter()
            .map(|m| {
                let n = self.channel_doc(&m.channel_id).map(|c| c.name).unwrap_or_else(|_| m.channel_id.clone());
                format!("{n}: {}", text::excerpt(&m.sender, 40))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The agent id of the session of a context: `channel:ro|ask:<assistant>`, from the channels as they are now.
    fn context_session_agent(&self, ctx: &ChannelContext) -> Option<String> {
        let agent = self.context_agent(ctx)?;
        let kind = if self.context_kind(ctx) == RunKind::Ask { "ask" } else { "ro" };
        Some(format!("{AGENT_PREFIX}{kind}:{agent}"))
    }

    pub fn context_create(&self, c: ContextCreate) -> Result<ContextInfo> {
        let _g = self.0.channels.context_lock.lock().unwrap();
        let name = Self::context_name(&c.name)?;
        if self.contexts_raw().len() >= MAX_CONTEXTS {
            return Err(Error::Invalid(format!("al massimo {MAX_CONTEXTS} contesti condivisi")));
        }
        let members = self.context_check(c.members, None)?;
        let mut ctx = ChannelContext {
            id: new_id("ctx"),
            name,
            agent_id: String::new(),
            session_id: String::new(),
            members,
            created_at: now_rfc3339(),
        };
        let want = self
            .context_session_agent(&ctx)
            .ok_or_else(|| Error::Invalid("i canali del contesto non usano lo stesso assistente".into()))?;
        ctx.agent_id = self.context_agent(&ctx).unwrap_or_default();
        let session = self.create_session(&want, Some(format!("Conversazione condivisa · {}", text::excerpt(&ctx.name, 40))))?;
        ctx.session_id = session.id;
        self.0.store.put_doc("channel_context", &ctx.id, &ctx)?;
        self.audit_user("context.create", &format!("«{}» ({}): {}", ctx.name, ctx.id, self.context_label(&ctx.members)));
        Ok(self.context_info(&ctx))
    }

    pub fn context_patch(&self, id: &str, p: ContextPatch) -> Result<ContextInfo> {
        let _g = self.0.channels.context_lock.lock().unwrap();
        let mut ctx = self.context_doc(id)?;
        if let Some(n) = p.name.as_deref() {
            ctx.name = Self::context_name(n)?;
        }
        let before = self.context_label(&ctx.members);
        if let Some(m) = p.members {
            ctx.members = self.context_check(m, Some(id))?;
            ctx.agent_id = self.context_agent(&ctx).unwrap_or_default();
        }
        self.0.store.put_doc("channel_context", &ctx.id, &ctx)?;
        self.audit_user("context.update", &format!("«{}» ({}): {} → {}", ctx.name, ctx.id, before, self.context_label(&ctx.members)));
        Ok(self.context_info(&ctx))
    }

    /// Deletes the context. Its session stays (it is a plain session of the app), and every account goes back to the
    /// conversation of its own channel.
    pub fn context_delete(&self, id: &str) -> Result<()> {
        let _g = self.0.channels.context_lock.lock().unwrap();
        let ctx = self.context_doc(id)?;
        self.0.store.delete_doc("channel_context", id)?;
        self.audit_user("context.delete", &format!("«{}» ({})", ctx.name, ctx.id));
        Ok(())
    }

    /// Takes out of the contexts the accounts of this channel that can no longer be there (the channel is gone, or the
    /// account is not on its list any more). A context left with nobody is deleted; its session stays.
    pub(super) fn contexts_prune(&self, channel_id: &str) {
        let _g = self.0.channels.context_lock.lock().unwrap();
        for mut ctx in self.contexts_raw() {
            if !ctx.members.iter().any(|m| m.channel_id == channel_id) {
                continue;
            }
            let before = ctx.members.len();
            ctx.members.retain(|m| {
                m.channel_id != channel_id || self.channel_doc(channel_id).is_ok_and(|c| c.allowed.contains(&norm(&c.kind, &m.sender)))
            });
            if ctx.members.len() == before {
                continue;
            }
            if ctx.members.is_empty() {
                let _ = self.0.store.delete_doc("channel_context", &ctx.id);
                self.audit_user("context.delete", &format!("«{}» ({}): non è rimasto nessun account", ctx.name, ctx.id));
            } else {
                let _ = self.0.store.put_doc("channel_context", &ctx.id, &ctx);
                self.audit_user("context.update", &format!("«{}» ({}): un account è uscito dal canale", ctx.name, ctx.id));
            }
        }
    }

    /// A channel that is in a context with others cannot change assistant: the answers would differ by channel.
    pub(super) fn context_guard_agent(&self, channel_id: &str, new_agent: &str) -> Result<()> {
        for ctx in self.contexts_raw() {
            if ctx.members.len() > 1
                && ctx.members.iter().any(|m| m.channel_id == channel_id)
                && self.context_agent(&ctx).is_some_and(|a| a != new_agent)
            {
                return Err(Error::Conflict(format!(
                    "il canale fa parte del contesto condiviso «{}», che usa un solo assistente per tutti i canali: scollegalo prima di cambiare assistente",
                    ctx.name
                )));
            }
        }
        Ok(())
    }

    /// The shared context a message belongs to, if any. A message belongs to one only when it is **direct**, comes from an
    /// account the owner linked, that account can take part now, and all the channels of the context agree on the
    /// assistant. In every other case the message keeps the conversation of its own channel.
    pub(super) fn context_for_message(&self, ch: &Channel, msg: &Inbound) -> Option<ChannelContext> {
        if !msg.direct {
            return None;
        }
        let me = ContextMember { channel_id: ch.id.clone(), sender: norm(&ch.kind, msg.senders.first()?) };
        let ctx = self.contexts_raw().into_iter().find(|c| c.members.contains(&me))?;
        if self.member_problem(&me).is_some() {
            return None;
        }
        let agent_id = self.context_agent(&ctx)?;
        Some(ChannelContext { agent_id, ..ctx })
    }

    /// The session of a context, with the restrictions of the moment: it is set at every turn, so a channel that was
    /// made read-only a minute ago already holds the whole context back. A session the owner deleted is replaced by an
    /// empty one.
    pub(super) fn context_session(&self, ctx: &ChannelContext) -> Result<String> {
        let want = self
            .context_session_agent(ctx)
            .ok_or_else(|| Error::Conflict("i canali del contesto non usano lo stesso assistente".into()))?;
        if let Ok(mut s) = self.0.store.session(&ctx.session_id) {
            if s.agent_id != want {
                s.agent_id = want;
                self.0.store.upsert_session(&s)?;
            }
            return Ok(ctx.session_id.clone());
        }
        let _g = self.0.channels.context_lock.lock().unwrap();
        // Someone else may have replaced it while we waited for the lock.
        let mut stored = self.context_doc(&ctx.id)?;
        if let Ok(mut s) = self.0.store.session(&stored.session_id) {
            if s.agent_id != want {
                s.agent_id = want;
                self.0.store.upsert_session(&s)?;
            }
            return Ok(stored.session_id);
        }
        let s = self.create_session(&want, Some(format!("Conversazione condivisa · {}", text::excerpt(&ctx.name, 40))))?;
        stored.session_id = s.id.clone();
        self.0.store.put_doc("channel_context", &stored.id, &stored)?;
        Ok(s.id)
    }
}
