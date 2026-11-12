//! Which scope a route needs: the one table the authentication middleware consults.
//!
//! A token carries scopes (`aiw_engine::tokens::Scope`); the owner token and a request from this
//! computer without a token are `full`. Every guarded route (see [`crate::security::is_guarded`])
//! has a minimum scope here:
//!
//! * `Full`: what changes this computer, its secrets or what the assistant may do: machine-level
//!   settings and `config` writes, secrets and provider keys, MCP server / hook / policy / agent /
//!   profile edits, channel and mail configuration, running and installing runtimes, downloads,
//!   training, anything that takes a path on this computer (the documents «path» import checks it
//!   again), token management and TLS.
//! * `Device`: what a paired phone or computer does: chat and sessions, approvals, memories,
//!   knowledge base, voice, the lists of models, agents, workflows and settings (read-only), and the
//!   surfaces of the next three scopes too.
//! * `Gateway`: the OpenAI/Anthropic-compatible `/v1/*` endpoints, for third-party apps.
//! * `A2a` / `Mcp`: `/a2a` and `/mcp` only.
//!
//! A route with no entry is **denied to everything but `full`**; a test walks the routes registered
//! in `lib.rs` and fails when one has no entry, so a new route has to be classified on purpose.

use aiw_engine::tokens::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Required {
    /// Needs no token (pairing: its own code is the credential).
    Public,
    /// Any valid token, whatever its scope (a token dropping itself).
    AnyToken,
    Scope(Scope),
}

use Required::{AnyToken, Public, Scope as S};
const FULL: Required = S(Scope::Full);
const DEVICE: Required = S(Scope::Device);
const GATEWAY: Required = S(Scope::Gateway);
const A2A: Required = S(Scope::A2a);
const MCP: Required = S(Scope::Mcp);

/// `(methods, path pattern, minimum scope)`; first match wins. `*` matches one path segment, `**`
/// the rest of the path; methods are `|`-separated, `*` is every method (`HEAD` counts as `GET`).
const RULES: &[(&str, &str, Required)] = &[
    // Pairing, and the token dropping itself.
    ("POST", "/api/pair", Public),
    ("DELETE", "/api/auth/token", AnyToken),
    // Token management and TLS: the owner only.
    ("*", "/api/tokens", FULL),
    ("*", "/api/tokens/**", FULL),
    ("*", "/api/tls", FULL),
    ("*", "/api/tls/**", FULL),
    // The surfaces of third-party apps and paired computers.
    ("*", "/v1/**", GATEWAY),
    ("*", "/a2a", A2A),
    ("*", "/mcp", MCP),
    // System.
    ("GET", "/api/system", DEVICE),
    ("POST", "/api/kill", DEVICE),
    ("GET", "/api/update", DEVICE),
    ("*", "/api/update/**", FULL),
    // Runtimes.
    ("GET", "/api/runtimes", DEVICE),
    ("POST", "/api/runtimes/refresh", DEVICE),
    ("*", "/api/runtimes/**", FULL),
    // Models.
    ("GET", "/api/models", DEVICE),
    ("POST", "/api/model-preview", DEVICE),
    ("GET", "/api/models/catalog", DEVICE),
    ("GET", "/api/recommend", DEVICE),
    ("GET", "/api/route", DEVICE),
    ("GET", "/api/models/providers", DEVICE),
    ("GET", "/api/models/search", DEVICE),
    ("GET", "/api/models/remote", DEVICE),
    ("GET", "/api/models/downloads", DEVICE),
    ("*", "/api/models/**", FULL),
    // Profiles, agents, teams, remote agents.
    ("GET", "/api/profiles", DEVICE),
    ("*", "/api/profiles/**", FULL),
    ("GET", "/api/agents", DEVICE),
    ("GET", "/api/agents/*/routing", DEVICE),
    ("*", "/api/agents/**", FULL),
    ("GET", "/api/remote-agents", DEVICE),
    ("*", "/api/remote-agents/**", FULL),
    ("GET", "/api/teams", DEVICE),
    ("GET", "/api/teams/*", DEVICE),
    ("*", "/api/teams/**", FULL),
    // Policy and hooks: what the assistant may do.
    ("GET", "/api/policy", DEVICE),
    ("*", "/api/policy", FULL),
    // Pure: it only tells how a command line is split into simple commands.
    ("POST", "/api/policy/analyze", DEVICE),
    ("*", "/api/hooks", FULL),
    // Sessions and runs.
    ("*", "/api/sessions", DEVICE),
    ("DELETE", "/api/sessions/*/worktree", FULL),
    ("POST", "/api/sessions/*/worktree/apply", FULL),
    ("*", "/api/sessions/**", DEVICE),
    ("POST", "/api/runs/*/rewind", FULL),
    ("*", "/api/runs/**", DEVICE),
    ("*", "/api/approvals", DEVICE),
    ("*", "/api/approvals/**", DEVICE),
    ("*", "/api/proposals", DEVICE),
    ("*", "/api/proposals/**", DEVICE),
    // Learning, memory, knowledge base (the handlers refuse path imports to anyone who is not local).
    ("*", "/api/lessons", DEVICE),
    ("*", "/api/lessons/**", DEVICE),
    ("*", "/api/memories", DEVICE),
    ("*", "/api/memories/**", DEVICE),
    ("*", "/api/kb/**", DEVICE),
    // Channels and mail: configuration of this computer's accounts.
    ("*", "/api/channels", FULL),
    ("*", "/api/channels/**", FULL),
    ("*", "/api/connectors/**", FULL),
    // The proactive assistant.
    ("*", "/api/proactive/**", DEVICE),
    // Workflows.
    ("GET", "/api/workflows", DEVICE),
    ("GET", "/api/workflows/runs", DEVICE),
    ("POST", "/api/workflows/*/run", DEVICE),
    ("*", "/api/workflows/**", FULL),
    // Device mesh: this computer's own paired devices.
    ("*", "/api/mesh", FULL),
    ("*", "/api/mesh/**", FULL),
    ("*", "/api/pairing/qr", FULL),
    // Voice.
    ("*", "/api/voice", DEVICE),
    ("*", "/api/voice/**", DEVICE),
    // Tools and MCP servers.
    ("GET", "/api/tools", DEVICE),
    ("*", "/api/tools/**", FULL),
    ("GET", "/api/mcp/servers", DEVICE),
    ("*", "/api/mcp/servers", FULL),
    ("*", "/api/mcp/servers/**", FULL),
    ("GET", "/api/mcp/prompts", DEVICE),
    ("POST", "/api/mcp/prompts/get", DEVICE),
    ("GET", "/api/mcp/elicitations", DEVICE),
    ("POST", "/api/mcp/elicitations/*", DEVICE),
    ("*", "/api/mcp/**", FULL),
    // Settings, activity log, exports.
    ("GET", "/api/config", DEVICE),
    ("*", "/api/config", FULL),
    ("GET", "/api/audit", DEVICE),
    ("*", "/api/traces/**", FULL),
    // Evaluation, optimisation, training.
    ("GET", "/api/evals", DEVICE),
    ("GET", "/api/evals/*", DEVICE),
    ("POST", "/api/evals/*/runs", DEVICE),
    ("POST", "/api/evals/*/reward", DEVICE),
    ("*", "/api/evals/**", FULL),
    ("GET", "/api/prompt-optimizations", DEVICE),
    ("GET", "/api/prompt-optimizations/*", DEVICE),
    ("*", "/api/prompt-optimizations/**", FULL),
    ("GET", "/api/eval-runs", DEVICE),
    ("GET", "/api/eval-runs/**", DEVICE),
    ("GET", "/api/trainers", DEVICE),
    ("*", "/api/trainers/**", FULL),
    ("GET", "/api/train/doctor", DEVICE),
    ("GET", "/api/train/jobs", DEVICE),
    ("GET", "/api/train/jobs/*", DEVICE),
    ("GET", "/api/train/jobs/*/log", DEVICE),
    ("*", "/api/train/**", FULL),
    ("GET", "/api/datasets", DEVICE),
    ("GET", "/api/datasets/*", DEVICE),
    ("*", "/api/datasets", FULL),
    ("*", "/api/datasets/**", FULL),
    ("*", "/api/dataset-import", FULL),
    ("GET", "/api/adapters", DEVICE),
    ("GET", "/api/adapters/*", DEVICE),
    ("*", "/api/adapters/**", FULL),
];

fn matches_pattern(pattern: &str, path: &str) -> bool {
    let mut pat = pattern.split('/');
    let mut seg = path.split('/');
    loop {
        match (pat.next(), seg.next()) {
            (None, None) => return true,
            (Some("**"), _) => return true,
            (Some("*"), Some(s)) if !s.is_empty() => {}
            (Some(p), Some(s)) if p == s => {}
            _ => return false,
        }
    }
}

fn matches_method(methods: &str, method: &str) -> bool {
    let method = if method.eq_ignore_ascii_case("HEAD") { "GET" } else { method };
    methods == "*" || methods.split('|').any(|m| m.eq_ignore_ascii_case(method))
}

/// The scope the route needs, or `None` when the table has no decision for it (the middleware then
/// asks for `full`).
pub(crate) fn required_scope(method: &str, path: &str) -> Option<Required> {
    let path = if path.len() > 1 { path.trim_end_matches('/') } else { path };
    RULES.iter().find(|(m, p, _)| matches_method(m, method) && matches_pattern(p, path)).map(|(_, _, r)| *r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches_pattern("/api/sessions/*/messages", "/api/sessions/s1/messages"));
        assert!(!matches_pattern("/api/sessions/*/messages", "/api/sessions//messages"));
        assert!(!matches_pattern("/api/sessions/*", "/api/sessions/a/b"));
        assert!(matches_pattern("/api/models/**", "/api/models/hf-cache:org/repo/file.gguf"));
        assert!(matches_pattern("/v1/**", "/v1/chat/completions"));
        assert!(!matches_pattern("/v1/**", "/v2/x"));
        assert!(matches_pattern("/api/tokens/**", "/api/tokens/tok_1/rotate"));
        assert!(!matches_pattern("/api/tokens", "/api/tokens/x"));
    }

    #[test]
    fn first_match_wins_and_methods_are_distinguished() {
        assert_eq!(required_scope("GET", "/api/models"), Some(DEVICE));
        assert_eq!(required_scope("DELETE", "/api/models/some/model.gguf"), Some(FULL));
        assert_eq!(required_scope("GET", "/api/models/catalog"), Some(DEVICE));
        assert_eq!(required_scope("PUT", "/api/config"), Some(FULL));
        assert_eq!(required_scope("GET", "/api/config"), Some(DEVICE));
        assert_eq!(required_scope("HEAD", "/api/config"), Some(DEVICE));
        assert_eq!(required_scope("POST", "/api/sessions/s/runs"), Some(DEVICE));
        assert_eq!(required_scope("POST", "/api/sessions/s/worktree/apply"), Some(FULL));
        assert_eq!(required_scope("POST", "/v1/chat/completions"), Some(GATEWAY));
        assert_eq!(required_scope("POST", "/mcp"), Some(MCP));
        assert_eq!(required_scope("POST", "/api/pair"), Some(Public));
        assert_eq!(required_scope("GET", "/api/tokens/"), Some(FULL), "a trailing slash does not change the decision");
        assert_eq!(required_scope("PATCH", "/api/nothing-here"), None);
    }

    /// Adding or changing an MCP server (it can run a command on this computer), confirming its command
    /// and reconnecting it are the owner's: no paired device, gateway, A2A or MCP token gets there.
    #[test]
    fn mcp_servers_can_only_be_changed_with_the_owner_scope() {
        for (method, path) in [
            ("POST", "/api/mcp/servers"),
            ("PUT", "/api/mcp/servers"),
            ("DELETE", "/api/mcp/servers/s1"),
            ("POST", "/api/mcp/servers/s1/approve"),
            ("POST", "/api/mcp/servers/s1/reconnect"),
            ("POST", "/api/mcp/servers/s1/oauth/start"),
            ("DELETE", "/api/mcp/servers/s1/oauth"),
            ("PUT", "/api/config"),
            ("PUT", "/api/policy"),
        ] {
            assert_eq!(required_scope(method, path), Some(FULL), "{method} {path}");
        }
        // A device may see the list, not change it.
        assert_eq!(required_scope("GET", "/api/mcp/servers"), Some(DEVICE));
        assert_eq!(required_scope("POST", "/api/policy/analyze"), Some(DEVICE));
    }
}
