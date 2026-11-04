//! Code mode (pattern P13): the model writes a short Rhai script that orchestrates tools in a
//! single step (loops, filters, aggregation) instead of one round trip per call.
//!
//! The interpreter is hermetic: no file system, network, process or module access of its own.
//! `tool(name, #{…})` is the only way out and every call goes back to the agent loop, through
//! the same policy, approval, hook and audit path as a direct tool call. Operations, call depth,
//! sizes and wall time are bounded.

use aiw_runtimes::ToolSpec;
use rhai::{Dynamic, Engine, EvalAltResult, Map, Position};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

pub const TOOL: &str = "code";
const MAX_TOOL_CALLS: usize = 200;
const MAX_OUTPUT: usize = 20_000;

/// A tool call requested by the script, answered by the agent loop.
pub struct Request {
    pub name: String,
    pub args: Value,
    pub reply: std::sync::mpsc::Sender<Result<String, String>>,
}

pub fn spec() -> ToolSpec {
    ToolSpec::function(
        TOOL,
        "Esegue uno script Rhai (sintassi simile a Rust/JavaScript) per orchestrare più strumenti in un solo passo: cicli, filtri, conteggi. \
         Lo script non ha accesso diretto a file o rete: chiama gli strumenti con tool(\"nome\", #{arg: valore}) (restituisce il testo dell'output, \
         lancia un errore se lo strumento fallisce; usa try/catch per gestirlo), print(x) per annotare, from_json(s)/to_json(v) per i dati. \
         Il risultato è il valore dell'ultima espressione. Esempio: \
         let files = tool(\"glob\", #{pattern: \"**/*.pdf\"}).split(\"\\n\"); let n = 0; for f in files { if f.contains(\"2025\") { n += 1; } } `PDF del 2025: ${n}`",
        json!({"type": "object", "properties": {"script": {"type": "string", "description": "Script Rhai"}}, "required": ["script"]}),
    )
}

fn err(msg: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(Dynamic::from(msg.into()), Position::NONE))
}

/// Runs `script` on the current (dedicated) thread. Tool calls are sent through `calls`.
pub fn run(
    script: &str,
    calls: tokio::sync::mpsc::UnboundedSender<Request>,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<String, String> {
    let mut engine = Engine::new();
    // Bounds: the script must finish quickly and cannot exhaust memory.
    engine.set_max_operations(5_000_000);
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_string_size(1 << 20);
    engine.set_max_array_size(20_000);
    engine.set_max_map_size(20_000);
    // Safety: Rhai's `eval` would let a script compile and run code built at run time, outside
    // these limits' intent; disabling the symbol keeps scripts to the code the model wrote.
    engine.disable_symbol("eval");
    let started = Instant::now();
    engine.on_progress(move |_| {
        if cancel.is_cancelled() {
            Some(Dynamic::from("annullato"))
        } else if started.elapsed() > timeout {
            Some(Dynamic::from(format!("tempo massimo di {} s superato", timeout.as_secs())))
        } else {
            None
        }
    });
    let printed = Arc::new(Mutex::new(String::new()));
    let sink = printed.clone();
    engine.on_print(move |s| {
        let mut p = sink.lock().unwrap();
        if p.len() < MAX_OUTPUT {
            p.push_str(s);
            p.push('\n');
        }
    });
    engine.on_debug(|_, _, _| {});
    let count = Arc::new(Mutex::new(0usize));
    let tool_fn = move |name: &str, args: Map| -> Result<String, Box<EvalAltResult>> {
        {
            let mut c = count.lock().unwrap();
            *c += 1;
            if *c > MAX_TOOL_CALLS {
                return Err(err(format!("troppe chiamate a strumenti (massimo {MAX_TOOL_CALLS})")));
            }
        }
        let args: Value = rhai::serde::from_dynamic(&Dynamic::from_map(args)).map_err(|e| err(format!("argomenti non validi: {e}")))?;
        let (tx, rx) = std::sync::mpsc::channel();
        calls.send(Request { name: name.to_string(), args, reply: tx }).map_err(|_| err("agente terminato"))?;
        match rx.recv() {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(e)) => Err(err(format!("{name}: {e}"))),
            Err(_) => Err(err("agente terminato")),
        }
    };
    let no_args = tool_fn.clone();
    engine.register_fn("tool", tool_fn);
    engine.register_fn("tool", move |name: &str| no_args(name, Map::new()));
    engine.register_fn("from_json", |s: &str| -> Result<Dynamic, Box<EvalAltResult>> {
        let v: Value = serde_json::from_str(s).map_err(|e| err(format!("JSON non valido: {e}")))?;
        rhai::serde::to_dynamic(v)
    });
    engine.register_fn("to_json", |d: Dynamic| -> Result<String, Box<EvalAltResult>> {
        let v: Value = rhai::serde::from_dynamic(&d)?;
        Ok(v.to_string())
    });
    let result = engine.eval::<Dynamic>(script);
    let printed = printed.lock().unwrap().clone();
    match result {
        Ok(v) => {
            let value = if v.is_unit() {
                String::new()
            } else if v.is_string() {
                v.into_string().unwrap_or_default()
            } else {
                v.to_string()
            };
            let mut out = printed;
            if !value.is_empty() {
                if !out.is_empty() {
                    out.push_str("---\n");
                }
                out.push_str(&value);
            }
            Ok(if out.trim().is_empty() { "(nessun risultato)".into() } else { out.chars().take(MAX_OUTPUT).collect() })
        }
        Err(e) => {
            let msg = match *e {
                EvalAltResult::ErrorRuntime(v, pos) if pos.is_none() => v.to_string(),
                EvalAltResult::ErrorTerminated(v, _) => format!("interrotto: {v}"),
                other => other.to_string(),
            };
            Err(if printed.is_empty() { msg } else { format!("{msg}\nOutput prima dell'errore:\n{printed}") })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves tool calls from a fixed table, like the agent loop would.
    fn run_with_tools(script: &str, answer: fn(&str, &Value) -> Result<String, String>) -> (Result<String, String>, Vec<String>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Request>();
        let script = script.to_string();
        let h = std::thread::spawn(move || run(&script, tx, CancellationToken::new(), Duration::from_secs(5)));
        let mut seen = Vec::new();
        while let Some(r) = rx.blocking_recv() {
            seen.push(format!("{} {}", r.name, r.args));
            let _ = r.reply.send(answer(&r.name, &r.args));
        }
        (h.join().unwrap(), seen)
    }

    #[test]
    fn scripts_call_tools_and_aggregate() {
        let (out, seen) = run_with_tools(
            r#"
            let files = tool("glob", #{pattern: "**/*.pdf"}).split("\n");
            let n = 0;
            for f in files { if f.contains("2025") { n += 1; } }
            print(`trovati ${files.len()} file`);
            let meta = from_json(tool("stat", #{path: files[0]}));
            `PDF del 2025: ${n}, primo: ${meta.size} byte`
            "#,
            |name, args| match name {
                "glob" => Ok("a-2025.pdf\nb-2024.pdf\nc-2025.pdf".into()),
                "stat" => Ok(format!("{{\"size\": 12, \"path\": {}}}", args["path"])),
                _ => Err("sconosciuto".into()),
            },
        );
        let out = out.unwrap();
        assert!(out.contains("trovati 3 file"), "{out}");
        assert!(out.ends_with("PDF del 2025: 2, primo: 12 byte"), "{out}");
        assert_eq!(seen[0], r#"glob {"pattern":"**/*.pdf"}"#);
    }

    #[test]
    fn tool_errors_can_be_caught_and_limits_hold() {
        let (out, _) = run_with_tools(
            r#"let r = "x"; try { r = tool("shell", #{command: "rm -rf /"}); } catch (e) { r = `negato: ${e}`; } r"#,
            |_, _| Err("Negato dalla policy".into()),
        );
        assert!(out.unwrap().contains("negato: shell: Negato dalla policy"));
        // Endless loops hit the operation budget instead of hanging the agent.
        let (out, _) = run_with_tools("let x = 0; loop { x += 1; }", |_, _| Ok(String::new()));
        assert!(out.is_err());
        // The `eval` symbol is disabled (see `run`).
        let (out, _) = run_with_tools(r#"eval("1 + 1")"#, |_, _| Ok(String::new()));
        assert!(out.is_err(), "eval must stay disabled");
    }
}
