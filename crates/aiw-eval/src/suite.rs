//! The suite file: tasks, the files they start from and the graders that decide whether they passed.

use aiw_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Time given to a task that does not say otherwise.
pub const DEFAULT_TIMEOUT_S: u64 = 300;
/// Longest timeout of a task or of a `command` grader.
pub const MAX_TIMEOUT_S: u64 = 3600;
/// Time given to a `command` grader that does not say otherwise.
pub const DEFAULT_COMMAND_TIMEOUT_S: u64 = 30;
/// Score a `judge` grader needs to pass when the suite does not say otherwise.
pub const DEFAULT_JUDGE_THRESHOLD: f64 = 0.7;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Agent of the tasks that do not name one (default: the `operator` agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Inference profile that replaces the agent's own, for the tasks that do not name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Files written in the working folder before the run (relative path → content).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
    pub graders: Vec<GraderSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraderSpec {
    /// The file exists in the working folder after the run.
    FileExists { path: String },
    /// The file exists and its text matches the regular expression.
    FileContains { path: String, pattern: String },
    /// The final answer matches the regular expression.
    OutputMatches { pattern: String },
    /// The final answer is JSON that conforms to the schema.
    JsonSchema { schema: Value },
    /// A shell command run in the working folder exits with the expected code. The suite author
    /// writes it and it runs with the user's own permissions, outside the agent's sandbox.
    Command {
        run: String,
        #[serde(default)]
        expect_exit: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_s: Option<u64>,
    },
    /// A model rates the work against the criteria (prompt, final answer and changed files are
    /// shown to it) and answers `{"score": 0..1, "reason": "…"}`; it passes from `threshold` up.
    Judge {
        criteria: String,
        /// Inference profile of the judge (default: the `default` profile).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        threshold: Option<f64>,
    },
}

impl GraderSpec {
    pub fn kind(&self) -> &'static str {
        match self {
            GraderSpec::FileExists { .. } => "file_exists",
            GraderSpec::FileContains { .. } => "file_contains",
            GraderSpec::OutputMatches { .. } => "output_matches",
            GraderSpec::JsonSchema { .. } => "json_schema",
            GraderSpec::Command { .. } => "command",
            GraderSpec::Judge { .. } => "judge",
        }
    }
}

impl Suite {
    /// Reads a suite from YAML or JSON text (JSON is YAML) and validates it.
    pub fn parse(text: &str) -> Result<Suite> {
        let suite: Suite = serde_yaml::from_str(text).map_err(|e| Error::Invalid(format!("suite non valida: {e}")))?;
        suite.validate()?;
        Ok(suite)
    }

    /// Reads a suite file (`.yaml`, `.yml` or `.json`).
    pub fn load_file(path: &Path) -> Result<Suite> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Everything that can be checked without running anything.
    pub fn validate(&self) -> Result<()> {
        check_id("la suite", &self.id)?;
        if self.name.trim().is_empty() {
            return Err(Error::Invalid("la suite deve avere un nome".into()));
        }
        if self.tasks.is_empty() {
            return Err(Error::Invalid("la suite deve avere almeno un task".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for t in &self.tasks {
            check_id("il task", &t.id)?;
            if !seen.insert(t.id.as_str()) {
                return Err(Error::Invalid(format!("id di task duplicato: {}", t.id)));
            }
            t.validate().map_err(|e| Error::Invalid(format!("task «{}»: {e}", t.id)))?;
        }
        Ok(())
    }
}

impl Task {
    fn validate(&self) -> Result<()> {
        if self.prompt.trim().is_empty() {
            return Err(Error::Invalid("serve un prompt".into()));
        }
        if let Some(t) = self.timeout_s {
            check_timeout("timeout_s", t)?;
        }
        for path in self.files.keys() {
            safe_relative(path).map_err(|e| Error::Invalid(format!("file «{path}»: {e}")))?;
        }
        if self.graders.is_empty() {
            return Err(Error::Invalid("serve almeno un grader (senza, il task passerebbe sempre)".into()));
        }
        for (i, g) in self.graders.iter().enumerate() {
            g.validate().map_err(|e| Error::Invalid(format!("grader {} ({}): {e}", i + 1, g.kind())))?;
        }
        Ok(())
    }
}

impl GraderSpec {
    fn validate(&self) -> Result<()> {
        match self {
            GraderSpec::FileExists { path } => safe_relative(path).map(|_| ()),
            GraderSpec::FileContains { path, pattern } => {
                safe_relative(path)?;
                check_regex(pattern)
            }
            GraderSpec::OutputMatches { pattern } => check_regex(pattern),
            GraderSpec::JsonSchema { schema } => {
                aiw_agent::schema::check_schema(schema).map_err(|e| Error::Invalid(format!("schema non valido: {e}")))
            }
            GraderSpec::Command { run, timeout_s, .. } => {
                if run.trim().is_empty() {
                    return Err(Error::Invalid("serve il comando da eseguire (`run`)".into()));
                }
                timeout_s.map_or(Ok(()), |t| check_timeout("timeout_s", t))
            }
            GraderSpec::Judge { criteria, threshold, .. } => {
                if criteria.trim().is_empty() {
                    return Err(Error::Invalid("servono i criteri di valutazione (`criteria`)".into()));
                }
                match threshold {
                    Some(t) if !(0.0..=1.0).contains(t) => Err(Error::Invalid("threshold deve essere tra 0 e 1".into())),
                    _ => Ok(()),
                }
            }
        }
    }
}

fn check_id(what: &str, id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!("id non valido per {what}: «{id}» (usa lettere, cifre, `-`, `_` e `.`, al massimo 64 caratteri)")))
    }
}

fn check_timeout(name: &str, t: u64) -> Result<()> {
    if (1..=MAX_TIMEOUT_S).contains(&t) {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{name} deve essere tra 1 e {MAX_TIMEOUT_S} secondi")))
    }
}

fn check_regex(pattern: &str) -> Result<()> {
    regex::Regex::new(pattern).map(|_| ()).map_err(|e| Error::Invalid(format!("espressione regolare non valida: {e}")))
}

/// A path that stays inside the working folder: relative, no `..`, no drive or root.
pub fn safe_relative(path: &str) -> Result<PathBuf> {
    if path.trim().is_empty() {
        return Err(Error::Invalid("percorso vuoto".into()));
    }
    let p = Path::new(path);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => {
                return Err(Error::Invalid(
                    "il percorso deve essere relativo e restare nella cartella di lavoro (niente `..` né percorsi assoluti)".into(),
                ))
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(Error::Invalid("percorso vuoto".into()));
    }
    Ok(out)
}

/// `rel` inside `root`, for reading what a run produced: a link that points outside is refused.
pub fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf> {
    let joined = root.join(safe_relative(rel)?);
    if let (Ok(real), Ok(base)) = (joined.canonicalize(), root.canonicalize()) {
        if !real.starts_with(&base) {
            return Err(Error::Invalid(format!("«{rel}» punta fuori dalla cartella di lavoro")));
        }
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
id: base
name: Suite di base
agent: operator
tasks:
  - id: crea-nota
    prompt: "Crea note.md con scritto ciao"
    files: { "input.txt": "contenuto iniziale" }
    timeout_s: 120
    graders:
      - { kind: file_exists, path: note.md }
      - { kind: file_contains, path: note.md, pattern: "(?i)ciao" }
      - { kind: output_matches, pattern: "note\\.md" }
      - { kind: json_schema, schema: { type: object, required: [ok] } }
      - { kind: command, run: "test -s note.md", expect_exit: 0, timeout_s: 30 }
"#;

    #[test]
    fn parses_the_documented_format() {
        let s = Suite::parse(YAML).unwrap();
        assert_eq!((s.id.as_str(), s.agent.as_deref()), ("base", Some("operator")));
        let t = &s.tasks[0];
        assert_eq!((t.timeout_s, t.files.len(), t.graders.len()), (Some(120), 1, 5));
        assert_eq!(
            t.graders.iter().map(GraderSpec::kind).collect::<Vec<_>>(),
            vec!["file_exists", "file_contains", "output_matches", "json_schema", "command"]
        );
        assert!(matches!(&t.graders[4], GraderSpec::Command { run, expect_exit: 0, timeout_s: Some(30) } if run == "test -s note.md"));
        // JSON is accepted too, and a suite survives a round trip.
        let again = Suite::parse(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again, s);
    }

    #[test]
    fn the_example_suite_is_valid() {
        let s = Suite::parse(include_str!("../../../docs/evals/esempio.yaml")).unwrap();
        assert_eq!(s.id, "esempio");
        assert!((3..=5).contains(&s.tasks.len()));
    }

    #[test]
    fn the_proactive_filter_suite_is_valid() {
        let s = Suite::parse(include_str!("../../../docs/evals/proattivo-filtro.yaml")).unwrap();
        assert_eq!(s.id, "proattivo-filtro");
        assert_eq!(s.tasks.len(), 6);
    }

    fn err(text: &str) -> String {
        Suite::parse(text).unwrap_err().to_string()
    }

    #[test]
    fn refuses_what_would_run_wrong() {
        let task = |extra: &str| format!("id: s\nname: S\ntasks:\n  - id: t\n    prompt: fai\n{extra}");
        assert!(err("id: s\nname: S\ntasks: []").contains("almeno un task"));
        assert!(err(&task("    graders: []")).contains("almeno un grader"));
        assert!(err(&task("    graders: [{kind: file_exists, path: ../x}]")).contains("relativo"));
        assert!(err(&task("    graders: [{kind: file_exists, path: /etc/passwd}]")).contains("relativo"));
        assert!(err(&task("    files: {'../x': a}\n    graders: [{kind: file_exists, path: x}]")).contains("file «../x»"));
        assert!(err(&task("    graders: [{kind: output_matches, pattern: '('}]")).contains("espressione regolare"));
        assert!(err(&task("    graders: [{kind: command, run: '  '}]")).contains("comando"));
        assert!(err(&task("    graders: [{kind: json_schema, schema: {type: ghost}}]")).contains("tipo `\"ghost\"`"));
        assert!(err(&task("    timeout_s: 0\n    graders: [{kind: file_exists, path: x}]")).contains("timeout_s"));
        assert!(err(&task("    graders: [{kind: judge, criteria: ' '}]")).contains("criteri"));
        assert!(err(&task("    graders: [{kind: judge, criteria: ok, threshold: 1.5}]")).contains("threshold"));
        assert!(err(&task("    graders: [{kind: telepatia}]")).contains("suite non valida"));
        assert!(err(&task("    colore: blu\n    graders: [{kind: file_exists, path: x}]")).contains("colore"));
        assert!(err("id: 'a b'\nname: S\ntasks: []").contains("id non valido"));
        assert!(err("id: s\nname: S\ntasks:\n  - {id: t, prompt: a, graders: [{kind: file_exists, path: x}]}\n  - {id: t, prompt: b, graders: [{kind: file_exists, path: x}]}").contains("duplicato"));
    }

    #[test]
    fn paths_stay_inside_the_working_folder() {
        assert_eq!(safe_relative("./a/b.txt").unwrap(), PathBuf::from("a/b.txt"));
        for bad in ["", "..", "a/../../b", "/abs", "a/../b"] {
            assert!(safe_relative(bad).is_err(), "{bad}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_inside(dir.path(), "x.txt").unwrap(), dir.path().join("x.txt"));
        #[cfg(unix)]
        {
            let out = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(out.path(), dir.path().join("link")).unwrap();
            assert!(resolve_inside(dir.path(), "link").is_err());
        }
    }
}
