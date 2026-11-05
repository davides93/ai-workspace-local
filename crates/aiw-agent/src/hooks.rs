//! Lifecycle hooks (pattern P08). A hook is a shell command receiving the event as JSON on
//! stdin. Exit code 2 (or `{"decision":"block","reason":…}` on stdout) blocks the action;
//! `{"updatedInput":{…}}` rewrites tool arguments; `{"additionalContext":"…"}` adds context.

use globset::Glob;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookConfig {
    /// SessionStart | UserPromptSubmit | PreToolUse | PostToolUse | Stop | SubagentStart | SubagentStop | PreCompact
    pub event: String,
    /// Glob on the tool name (tool events) or on the agent id (subagent events).
    #[serde(default)]
    pub matcher: Option<String>,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
}

fn default_timeout() -> u64 {
    30
}

/// The lifecycle events a hook can listen to.
pub const EVENTS: [&str; 8] =
    ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SubagentStart", "SubagentStop", "PreCompact"];
/// Longest timeout a hook can ask for, in seconds.
pub const MAX_TIMEOUT_S: u64 = 600;

impl HookConfig {
    /// Checks a hook that comes from outside (the API) and returns it trimmed: a known event, a
    /// command, a timeout of 1 to [`MAX_TIMEOUT_S`] seconds and a matcher that compiles as a glob
    /// (an empty matcher means "every tool" and is stored as `None`).
    pub fn validated(mut self) -> Result<Self, String> {
        if !EVENTS.contains(&self.event.as_str()) {
            return Err(format!("evento `{}` non valido: usa {}", self.event, EVENTS.join(", ")));
        }
        self.command = self.command.trim().to_string();
        if self.command.is_empty() {
            return Err("manca il comando".into());
        }
        if !(1..=MAX_TIMEOUT_S).contains(&self.timeout_s) {
            return Err(format!("timeout_s deve essere tra 1 e {MAX_TIMEOUT_S} secondi (trovato {})", self.timeout_s));
        }
        self.matcher = self.matcher.take().map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        if let Some(m) = self.matcher.as_deref().filter(|m| Glob::new(m).is_err()) {
            return Err(format!("il filtro `{m}` non è un glob valido"));
        }
        Ok(self)
    }
}

#[derive(Debug, Default, Clone)]
pub struct HookOutcome {
    pub block: Option<String>,
    pub updated_input: Option<Value>,
    pub context: Vec<String>,
}

pub async fn run_hooks(hooks: &[HookConfig], event: &str, tool: Option<&str>, payload: &Value) -> HookOutcome {
    let mut out = HookOutcome::default();
    for h in hooks.iter().filter(|h| h.event == event) {
        if let (Some(m), Some(t)) = (&h.matcher, tool) {
            if !Glob::new(m).map(|g| g.compile_matcher().is_match(t)).unwrap_or(m == t) {
                continue;
            }
        }
        match run_one(h, payload).await {
            Ok((code, stdout, stderr)) => {
                let parsed = serde_json::from_str::<Value>(stdout.trim()).ok();
                if code == 2 {
                    out.block = Some(if stderr.trim().is_empty() {
                        format!("bloccato dall'hook {}", h.command)
                    } else {
                        stderr.trim().to_string()
                    });
                }
                if let Some(v) = parsed {
                    if v["decision"] == "block" {
                        out.block = Some(v["reason"].as_str().unwrap_or("bloccato da un hook").to_string());
                    }
                    if let Some(u) = v.get("updatedInput") {
                        out.updated_input = Some(u.clone());
                    }
                    if let Some(c) = v["additionalContext"].as_str() {
                        out.context.push(c.to_string());
                    }
                } else if code == 0 && !stdout.trim().is_empty() && matches!(event, "UserPromptSubmit" | "SessionStart") {
                    out.context.push(stdout.trim().to_string());
                }
                if out.block.is_some() {
                    break;
                }
            }
            Err(e) => tracing::warn!(hook = %h.command, "hook failed: {e}"),
        }
    }
    out
}

async fn run_one(h: &HookConfig, payload: &Value) -> std::io::Result<(i32, String, String)> {
    use tokio::io::AsyncWriteExt;
    let (sh, flag) = if cfg!(windows) { ("powershell", "-Command") } else { ("/bin/sh", "-c") };
    let mut child = tokio::process::Command::new(sh)
        .arg(flag)
        .arg(&h.command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes()).await;
    }
    let out = tokio::time::timeout(Duration::from_secs(h.timeout_s), child.wait_with_output())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "hook timeout"))??;
    Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into(), String::from_utf8_lossy(&out.stderr).into()))
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    fn hook(event: &str, command: &str, timeout_s: u64) -> HookConfig {
        HookConfig { event: event.into(), matcher: None, command: command.into(), timeout_s }
    }

    #[test]
    fn external_hooks_are_validated_and_trimmed() {
        for event in EVENTS {
            assert!(hook(event, "echo ok", 30).validated().is_ok(), "{event}");
        }
        let h = HookConfig { matcher: Some(" shell ".into()), ..hook("PreToolUse", "  ./check.sh  ", 600) }.validated().unwrap();
        assert_eq!((h.matcher.as_deref(), h.command.as_str(), h.timeout_s), (Some("shell"), "./check.sh", 600));
        assert_eq!(HookConfig { matcher: Some("  ".into()), ..hook("Stop", "x", 1) }.validated().unwrap().matcher, None);
        for bad in [
            hook("Nope", "echo", 30),
            hook("pretooluse", "echo", 30),
            hook("", "echo", 30),
            hook("Stop", "", 30),
            hook("Stop", "   ", 30),
            hook("Stop", "echo", 0),
            hook("Stop", "echo", 601),
            HookConfig { matcher: Some("sh[ell".into()), ..hook("PreToolUse", "echo", 30) },
        ] {
            assert!(bad.clone().validated().is_err(), "{bad:?}");
        }
        assert!(hook("Nope", "echo", 30).validated().unwrap_err().contains("PreToolUse"));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn blocks_rewrites_and_adds_context() {
        let hooks = vec![
            HookConfig {
                event: "PreToolUse".into(),
                matcher: Some("shell".into()),
                command: "grep -q 'rm -rf' && { echo 'comando distruttivo' >&2; exit 2; } || exit 0".into(),
                timeout_s: 5,
            },
            HookConfig {
                event: "PreToolUse".into(),
                matcher: Some("write_*".into()),
                command: "echo '{\"updatedInput\":{\"path\":\"safe.txt\",\"content\":\"x\"}}'".into(),
                timeout_s: 5,
            },
            HookConfig { event: "UserPromptSubmit".into(), matcher: None, command: "echo 'Oggi è festivo'".into(), timeout_s: 5 },
        ];
        let o = run_hooks(&hooks, "PreToolUse", Some("shell"), &json!({"tool":"shell","args":{"command":"rm -rf /"}})).await;
        assert_eq!(o.block.as_deref(), Some("comando distruttivo"));
        let o = run_hooks(&hooks, "PreToolUse", Some("shell"), &json!({"tool":"shell","args":{"command":"ls"}})).await;
        assert!(o.block.is_none());
        let o = run_hooks(&hooks, "PreToolUse", Some("write_file"), &json!({})).await;
        assert_eq!(o.updated_input.unwrap()["path"], "safe.txt");
        let o = run_hooks(&hooks, "UserPromptSubmit", None, &json!({"prompt":"ciao"})).await;
        assert_eq!(o.context, vec!["Oggi è festivo".to_string()]);
    }
}
