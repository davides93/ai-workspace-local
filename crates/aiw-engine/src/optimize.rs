//! Prompt optimization driven by evaluation: the suite is run with the agent's current system
//! prompt (the baseline) and with variants the model writes after seeing what failed; the best
//! variant is only proposed. Applying it to the agent is always a separate step of the user.

use crate::eval::EvalRequest;
use crate::Engine;
use aiw_core::api::{PromptOptimization, PromptVariant};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_eval::{EvalHost, EvalReport, Suite};
use serde_json::{json, Value};

const OPTIMIZATION_KIND: &str = "prompt_optimization";
/// Most optimizations kept in the list the API and the CLI show.
const MAX_OPTIMIZATIONS: usize = 100;
pub const DEFAULT_VARIANTS: u32 = 3;
pub const MAX_VARIANTS: u32 = 5;
/// Failed attempts described to the model, and the length of each description.
const MAX_FAILURES: usize = 8;
const FAILURE_CHARS: usize = 600;
/// Longest system prompt a variant may have.
const MAX_PROMPT_CHARS: usize = 12_000;
/// Lines of context around a change in the diff.
const DIFF_CONTEXT: usize = 3;
/// Prompts with more lines than this are shown as fully replaced instead of compared.
const DIFF_MAX_LINES: usize = 1_500;

/// How to optimize (all optional).
#[derive(Debug, Clone, Default)]
pub struct OptimizeOptions {
    /// Variants to try, 1 to [`MAX_VARIANTS`] (default [`DEFAULT_VARIANTS`]).
    pub variants: Option<u32>,
    /// Inference profile the tasks run with; it also writes the variants when `learning.profile` is empty.
    pub profile: Option<String>,
    /// Times each task is run (default 1).
    pub repeat: Option<u32>,
}

/// Unified diff of two texts, line by line; empty when they are equal.
pub fn unified_diff(from: &str, to: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Op {
        Keep,
        Del,
        Add,
    }
    if from == to {
        return String::new();
    }
    let a: Vec<&str> = from.lines().collect();
    let b: Vec<&str> = to.lines().collect();
    let mut ops: Vec<(Op, &str)> = Vec::new();
    if a.len() > DIFF_MAX_LINES || b.len() > DIFF_MAX_LINES {
        ops.extend(a.iter().map(|l| (Op::Del, *l)));
        ops.extend(b.iter().map(|l| (Op::Add, *l)));
    } else {
        // Longest common subsequence of the lines, from the end so the walk below goes forward.
        let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                ops.push((Op::Keep, a[i]));
                i += 1;
                j += 1;
            } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
                ops.push((Op::Del, a[i]));
                i += 1;
            } else {
                ops.push((Op::Add, b[j]));
                j += 1;
            }
        }
    }
    // Hunks: every change with DIFF_CONTEXT lines around it, nearby changes merged.
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (op, _))| *op != Op::Keep).map(|(k, _)| k).collect();
    let mut out = String::from("--- attuale\n+++ variante\n");
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(DIFF_CONTEXT);
        let mut end = changed[k];
        while k + 1 < changed.len() && changed[k + 1] <= end + 2 * DIFF_CONTEXT + 1 {
            k += 1;
            end = changed[k];
        }
        k += 1;
        let end = (end + DIFF_CONTEXT).min(ops.len() - 1);
        let line_no = |upto: usize, side: Op| ops[..upto].iter().filter(|(op, _)| *op == Op::Keep || *op == side).count() + 1;
        let count = |side: Op| ops[start..=end].iter().filter(|(op, _)| *op == Op::Keep || *op == side).count();
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", line_no(start, Op::Del), count(Op::Del), line_no(start, Op::Add), count(Op::Add)));
        for (op, line) in &ops[start..=end] {
            out.push(match op {
                Op::Keep => ' ',
                Op::Del => '-',
                Op::Add => '+',
            });
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("… [troncato]");
    out
}

/// What went wrong in the failed attempts of a report, for the model that rewrites the prompt.
fn failure_details(report: &EvalReport, suite: &Suite) -> String {
    let mut out = String::new();
    for t in report.tasks.iter().filter(|t| !t.passed).take(MAX_FAILURES) {
        let prompt = suite.tasks.iter().find(|s| s.id == t.task_id).map(|s| s.prompt.as_str()).unwrap_or_default();
        out.push_str(&format!("## Task «{}»\nRichiesta: {}\n", t.task_id, truncate(prompt, FAILURE_CHARS)));
        if let Some(e) = &t.error {
            out.push_str(&format!("Errore del run: {}\n", truncate(e, FAILURE_CHARS)));
        }
        for g in t.graders.iter().filter(|g| !g.passed) {
            out.push_str(&format!("- grader {} non superato: {}\n", g.kind, truncate(&g.detail, FAILURE_CHARS)));
        }
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str("(nessun task fallito: cerca comunque un miglioramento di punteggio)\n");
    }
    out
}

fn variants_prompt(base: &str, failures: &str, n: u32) -> String {
    format!(
        "Devi migliorare il system prompt di un agente, valutato con una suite di task automatici. Scrivi {n} varianti diverse del prompt \
         che risolvano i fallimenti qui sotto senza peggiorare il resto: ogni variante è il testo COMPLETO del nuovo prompt, non una modifica.\n\n\
         Rispondi solo con un oggetto JSON, senza altro testo: {{\"variants\": [{{\"prompt\": \"testo completo\", \"rationale\": \"cosa cambia e perché, in una frase\"}}]}}\n\n\
         # Prompt attuale\n{}\n\n# Task falliti\n{failures}",
        if base.trim().is_empty() { "(vuoto: l'agente usa il prompt di base dell'applicazione)" } else { base }
    )
}

fn variants_schema(n: u32) -> Value {
    json!({
        "type": "object",
        "required": ["variants"],
        "properties": {
            "variants": {
                "type": "array",
                "minItems": 1,
                "maxItems": n,
                "items": {"type": "object", "required": ["prompt", "rationale"], "properties": {"prompt": {"type": "string"}, "rationale": {"type": "string"}}}
            }
        }
    })
}

/// The `(prompt, rationale)` pairs of the answer, validated; empty, repeated and unchanged prompts dropped.
fn parse_variants(answer: &str, base: &str, n: u32) -> std::result::Result<Vec<(String, String)>, String> {
    let v = aiw_agent::schema::extract_json(answer).ok_or_else(|| "la risposta non contiene un oggetto JSON".to_string())?;
    aiw_agent::schema::validate(&variants_schema(n), &v).map_err(|e| e.join("; "))?;
    let mut out: Vec<(String, String)> = Vec::new();
    for item in v["variants"].as_array().into_iter().flatten() {
        let prompt = item["prompt"].as_str().unwrap_or_default().trim().to_string();
        if prompt.is_empty() || prompt == base.trim() || out.iter().any(|(p, _)| *p == prompt) {
            continue;
        }
        if prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(format!("una variante supera i {MAX_PROMPT_CHARS} caratteri"));
        }
        out.push((prompt, item["rationale"].as_str().unwrap_or_default().trim().to_string()));
    }
    if out.is_empty() {
        return Err("nessuna variante utile: vuote o uguali al prompt attuale".into());
    }
    Ok(out)
}

/// Does `(score, pass_rate)` beat `than`? Pass rate only counts when the scores are equal.
fn beats(score: f64, pass_rate: f64, than: (f64, f64)) -> bool {
    const EPS: f64 = 1e-9;
    score > than.0 + EPS || ((score - than.0).abs() <= EPS && pass_rate > than.1 + EPS)
}

impl Engine {
    // ------------------------------------------------------------------ documents
    /// The optimizations, most recent first.
    pub fn prompt_optimizations(&self) -> Result<Vec<PromptOptimization>> {
        let mut all: Vec<PromptOptimization> = self.0.store.list_docs(OPTIMIZATION_KIND)?;
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        all.truncate(MAX_OPTIMIZATIONS);
        Ok(all)
    }
    pub fn prompt_optimization(&self, id: &str) -> Result<PromptOptimization> {
        self.0.store.get_doc(OPTIMIZATION_KIND, id)?.ok_or_else(|| Error::NotFound(format!("ottimizzazione del prompt {id} non trovata")))
    }
    fn save_optimization(&self, o: &PromptOptimization) {
        if let Err(e) = self.0.store.put_doc(OPTIMIZATION_KIND, &o.id, o) {
            tracing::warn!("ottimizzazione del prompt {}: {e}", o.id);
        }
    }
    /// Optimizations still `running` when the engine starts belong to a run that was cut short.
    pub(crate) fn fail_interrupted_optimizations(&self) -> Result<()> {
        for mut o in self.0.store.list_docs::<PromptOptimization>(OPTIMIZATION_KIND)?.into_iter().filter(|o| o.status == "running") {
            o.status = "failed".into();
            o.error = Some("interrotta: l'applicazione è stata chiusa durante l'ottimizzazione".into());
            o.finished_at = Some(now_rfc3339());
            self.save_optimization(&o);
        }
        Ok(())
    }

    // ------------------------------------------------------------------ run
    /// Checks the request and stores the optimization as `running`, with the baseline still to measure.
    fn begin_optimization(&self, suite: &Suite, agent_id: &str, opts: &OptimizeOptions) -> Result<PromptOptimization> {
        suite.validate()?;
        let agent = self.agent(agent_id)?;
        if let Some(p) = &opts.profile {
            self.profile(p)?;
        }
        let n = opts.variants.unwrap_or(DEFAULT_VARIANTS);
        if n == 0 || n > MAX_VARIANTS {
            return Err(Error::Invalid(format!("variants deve essere tra 1 e {MAX_VARIANTS}")));
        }
        let repeat = opts.repeat.unwrap_or(1);
        if repeat == 0 || repeat > aiw_eval::MAX_REPEAT {
            return Err(Error::Invalid(format!("repeat deve essere tra 1 e {}", aiw_eval::MAX_REPEAT)));
        }
        let o = PromptOptimization {
            id: new_id("opt"),
            suite_id: suite.id.clone(),
            agent_id: agent.id,
            status: "running".into(),
            started_at: now_rfc3339(),
            finished_at: None,
            base_prompt: agent.system_prompt,
            baseline_score: 0.0,
            baseline_pass_rate: 0.0,
            baseline_report_id: None,
            variants: vec![],
            best: None,
            applied: None,
            error: None,
        };
        self.save_optimization(&o);
        Ok(o)
    }

    /// Runs the optimization of `agent_id` on `suite` (a stored id or a file) to the end.
    pub async fn optimize_prompt(&self, suite: &str, agent_id: &str, opts: &OptimizeOptions) -> Result<PromptOptimization> {
        let suite = self.resolve_eval_suite(suite)?;
        let initial = self.begin_optimization(&suite, agent_id, opts)?;
        self.run_optimization(initial, &suite, opts).await
    }

    /// Starts the optimization in the background and returns it as it is (`running`): callers poll
    /// [`Engine::prompt_optimization`].
    pub fn start_prompt_optimization(&self, suite: Suite, agent_id: &str, opts: OptimizeOptions) -> Result<PromptOptimization> {
        let initial = self.begin_optimization(&suite, agent_id, &opts)?;
        let engine = self.clone();
        let started = initial.clone();
        tokio::spawn(async move {
            let _ = engine.run_optimization(started, &suite, &opts).await;
        });
        Ok(initial)
    }

    /// Whatever goes wrong ends in a stored `failed` optimization carrying the error, and in `Err`.
    async fn run_optimization(&self, mut o: PromptOptimization, suite: &Suite, opts: &OptimizeOptions) -> Result<PromptOptimization> {
        match self.optimization_steps(&mut o, suite, opts).await {
            Ok(()) => {
                o.status = "done".into();
                o.finished_at = Some(now_rfc3339());
                self.save_optimization(&o);
                Ok(o)
            }
            Err(e) => {
                o.status = "failed".into();
                o.error = Some(e.to_string());
                o.finished_at = Some(now_rfc3339());
                self.save_optimization(&o);
                Err(e)
            }
        }
    }

    async fn optimization_steps(&self, o: &mut PromptOptimization, suite: &Suite, opts: &OptimizeOptions) -> Result<()> {
        let n = opts.variants.unwrap_or(DEFAULT_VARIANTS);
        let request = |system_prompt: Option<String>| EvalRequest {
            agent: Some(o.agent_id.clone()),
            profile: opts.profile.clone(),
            repeat: opts.repeat,
            system_prompt,
            ..Default::default()
        };
        // (1) The baseline: the suite with the prompt the agent has now.
        let baseline = self.run_eval(suite, &request(None), &mut |_| {}).await?;
        o.baseline_score = baseline.summary.score;
        o.baseline_pass_rate = baseline.summary.pass_rate;
        o.baseline_report_id = Some(baseline.id.clone());
        self.save_optimization(o);

        // (2) The variants, written by the model from the failures of the baseline.
        let profile = match self.0.config.get_str("learning.profile").or_else(|| opts.profile.clone()) {
            Some(p) => self.profile(&p).map_err(|_| Error::Invalid(format!("il profilo di inferenza `{p}` per le varianti non esiste")))?,
            None => self.profile(&self.agent(&o.agent_id)?.inference_profile).or_else(|_| self.fallback_profile())?,
        };
        let prompt = variants_prompt(&o.base_prompt, &failure_details(&baseline, suite), n);
        let mut answer = self.complete(Some(&profile.id), &prompt).await?;
        let candidates = match parse_variants(&answer, &o.base_prompt, n) {
            Ok(c) => c,
            Err(first) => {
                let retry = format!(
                    "{prompt}\n\n# Correzione\nLa tua risposta precedente non era valida ({first}). Rispondi di nuovo solo con l'oggetto JSON richiesto."
                );
                answer = self.complete(Some(&profile.id), &retry).await?;
                parse_variants(&answer, &o.base_prompt, n)
                    .map_err(|e| Error::Invalid(format!("il modello non ha prodotto varianti valide del prompt: {e}")))?
            }
        };

        // (3) The suite once per variant.
        for (text, rationale) in candidates {
            let report = self.run_eval(suite, &request(Some(text.clone())), &mut |_| {}).await?;
            o.variants.push(PromptVariant {
                diff: unified_diff(&o.base_prompt, &text),
                prompt: text,
                rationale,
                score: report.summary.score,
                pass_rate: report.summary.pass_rate,
                report_id: Some(report.id),
            });
            self.save_optimization(o);
        }

        // (4) The best by score, then pass rate, then the shortest prompt; proposed only when it beats the baseline.
        let best = o
            .variants
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.score
                    .total_cmp(&b.score)
                    .then(a.pass_rate.total_cmp(&b.pass_rate))
                    .then(b.prompt.chars().count().cmp(&a.prompt.chars().count()))
            })
            .map(|(i, _)| i);
        o.best = best.filter(|i| beats(o.variants[*i].score, o.variants[*i].pass_rate, (o.baseline_score, o.baseline_pass_rate)));
        Ok(())
    }

    // ------------------------------------------------------------------ apply
    /// Saves the prompt of `variant` as the system prompt of the agent. It is refused when the
    /// agent's prompt is no longer the one the optimization started from.
    pub fn apply_prompt_optimization(&self, id: &str, variant: usize) -> Result<PromptOptimization> {
        let mut o = self.prompt_optimization(id)?;
        if o.status != "done" {
            return Err(Error::Conflict(match o.status.as_str() {
                "applied" => "questa ottimizzazione è già stata applicata".into(),
                "running" => "l'ottimizzazione è ancora in corso".into(),
                _ => "l'ottimizzazione non è finita: non c'è nulla da applicare".into(),
            }));
        }
        let v = o.variants.get(variant).ok_or_else(|| Error::Invalid(format!("la variante {variant} non esiste")))?;
        let mut agent = self.agent(&o.agent_id)?;
        if agent.system_prompt != o.base_prompt {
            return Err(Error::Conflict(format!(
                "il prompt dell'agente «{}» è cambiato dopo l'ottimizzazione: rifalla con il prompt attuale",
                agent.name
            )));
        }
        agent.system_prompt = v.prompt.clone();
        let agent = self.save_agent(agent)?;
        o.status = "applied".into();
        o.applied = Some(variant);
        self.save_optimization(&o);
        self.audit_user("prompt.apply", &format!("agente {} · ottimizzazione {} · variante {}", agent.id, o.id, variant + 1));
        Ok(o)
    }
}
