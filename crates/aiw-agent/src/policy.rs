//! Policy engine (ADR 0005): mode + `tool(pattern)` rules → allow / ask / deny.

use aiw_tools::Risk;
use globset::Glob;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask(String),
    Deny(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyRule {
    /// Glob on the tool name (`shell`, `mcp__github__*`).
    pub tool: String,
    /// Glob on the tool subject (path, command, url). `None` = any.
    #[serde(default)]
    pub pattern: Option<String>,
    /// allow | ask | deny
    pub action: String,
}

impl PolicyRule {
    /// Parses `shell(git status*)` / `write_file` style rules.
    pub fn parse(spec: &str, action: &str) -> Option<Self> {
        let spec = spec.trim();
        if let Some(open) = spec.find('(') {
            let close = spec.rfind(')')?;
            Some(Self {
                tool: spec[..open].trim().to_string(),
                pattern: Some(spec[open + 1..close].trim().to_string()),
                action: action.into(),
            })
        } else if !spec.is_empty() {
            Some(Self { tool: spec.to_string(), pattern: None, action: action.into() })
        } else {
            None
        }
    }

    fn matches(&self, tool: &str, subject: Option<&str>) -> bool {
        let g = |p: &str, v: &str| Glob::new(p).map(|g| g.compile_matcher().is_match(v)).unwrap_or(p == v);
        if !g(&self.tool, tool) {
            return false;
        }
        match (&self.pattern, subject) {
            (None, _) => true,
            (Some(p), Some(s)) => g(p, s) || s.starts_with(p.trim_end_matches('*')) && p.ends_with('*'),
            (Some(_), None) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Policy {
    /// plan | ask | auto-edit | auto | yolo
    pub mode: String,
    pub rules: Vec<PolicyRule>,
    /// The OS sandbox can actually contain shell commands on this machine.
    pub sandbox_available: bool,
}

impl Policy {
    pub fn evaluate(&self, tool: &str, risk: Risk, subject: Option<&str>, session_allowed: bool) -> Decision {
        for action in ["deny", "allow", "ask"] {
            if let Some(r) = self.rules.iter().find(|r| r.action == action && r.matches(tool, subject)) {
                return match action {
                    "deny" => {
                        Decision::Deny(format!("regola: {}{}", r.tool, r.pattern.as_ref().map(|p| format!("({p})")).unwrap_or_default()))
                    }
                    "allow" => Decision::Allow,
                    _ => Decision::Ask("regola di conferma".into()),
                };
            }
        }
        if session_allowed {
            return Decision::Allow;
        }
        let ask = |why: &str| Decision::Ask(why.to_string());
        match self.mode.as_str() {
            "yolo" => Decision::Allow,
            "plan" => match risk {
                Risk::Read | Risk::Network => Decision::Allow,
                _ => Decision::Deny("modalità piano: solo lettura, proponi le modifiche invece di eseguirle".into()),
            },
            "auto" => match risk {
                Risk::Computer => ask("azione su mouse e tastiera"),
                Risk::Exec if !self.sandbox_available => ask("comando senza sandbox su questo sistema"),
                _ => Decision::Allow,
            },
            "auto-edit" => match risk {
                Risk::Read | Risk::Network | Risk::Write => Decision::Allow,
                Risk::Exec => ask("esecuzione di un comando"),
                Risk::Computer => ask("azione su mouse e tastiera"),
            },
            _ => match risk {
                Risk::Read | Risk::Network => Decision::Allow,
                Risk::Write => ask("modifica di file"),
                Risk::Exec => ask("esecuzione di un comando"),
                Risk::Computer => ask("azione su mouse e tastiera"),
            },
        }
    }
}

pub fn risk_level(risk: Risk) -> &'static str {
    match risk {
        Risk::Read | Risk::Network => "low",
        Risk::Write => "medium",
        Risk::Exec | Risk::Computer => "high",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(mode: &str, rules: Vec<PolicyRule>) -> Policy {
        Policy { mode: mode.into(), rules, sandbox_available: true }
    }

    #[test]
    fn modes_map_risks() {
        assert_eq!(p("ask", vec![]).evaluate("read_file", Risk::Read, None, false), Decision::Allow);
        assert!(matches!(p("ask", vec![]).evaluate("write_file", Risk::Write, None, false), Decision::Ask(_)));
        assert!(matches!(p("plan", vec![]).evaluate("shell", Risk::Exec, None, false), Decision::Deny(_)));
        assert_eq!(p("auto-edit", vec![]).evaluate("edit_file", Risk::Write, None, false), Decision::Allow);
        assert!(matches!(p("auto-edit", vec![]).evaluate("shell", Risk::Exec, None, false), Decision::Ask(_)));
        assert_eq!(p("auto", vec![]).evaluate("shell", Risk::Exec, None, false), Decision::Allow);
        let no_sb = Policy { mode: "auto".into(), rules: vec![], sandbox_available: false };
        assert!(matches!(no_sb.evaluate("shell", Risk::Exec, None, false), Decision::Ask(_)));
        assert_eq!(p("yolo", vec![]).evaluate("computer_click", Risk::Computer, None, false), Decision::Allow);
        assert_eq!(p("ask", vec![]).evaluate("write_file", Risk::Write, None, true), Decision::Allow);
    }

    #[test]
    fn rules_override_modes_and_deny_wins() {
        let rules = vec![
            PolicyRule::parse("shell(git status*)", "allow").unwrap(),
            PolicyRule::parse("shell(rm *)", "deny").unwrap(),
            PolicyRule::parse("mcp__github__*", "ask").unwrap(),
        ];
        let pol = p("ask", rules);
        assert_eq!(pol.evaluate("shell", Risk::Exec, Some("git status --short"), false), Decision::Allow);
        assert!(matches!(pol.evaluate("shell", Risk::Exec, Some("rm -rf /"), false), Decision::Deny(_)));
        assert!(
            matches!(pol.evaluate("shell", Risk::Exec, Some("rm -rf /"), true), Decision::Deny(_)),
            "deny rules beat session approvals"
        );
        assert!(matches!(pol.evaluate("mcp__github__list_issues", Risk::Read, None, false), Decision::Ask(_)));
    }
}
