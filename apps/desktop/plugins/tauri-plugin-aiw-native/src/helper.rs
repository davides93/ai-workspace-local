//! Client of the `aiw-apple-fm` helper (macOS desktop): a small Swift executable that talks to the
//! Foundation Models framework and speaks one JSON object per line over stdin/stdout (see
//! `macos-helper/main.swift`). The module has no platform dependency, so the protocol is tested
//! everywhere with a fake helper.

use crate::{GenerateArgs, NativeEvent};
use aiw_core::{Error, Result};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// How long the helper may take to say whether the model is available.
const AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(5);
/// Last bytes of the helper's stderr kept for error messages.
const STDERR_TAIL: usize = 4096;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct HelperAvailability {
    pub available: bool,
    #[serde(default)]
    pub detail: Option<String>,
    /// Context window of the system model, in tokens.
    #[serde(default, rename = "contextTokens")]
    pub context_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Helper {
    path: PathBuf,
}

impl Helper {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Asks the helper whether the system model can be used right now.
    pub fn availability(&self) -> Result<HelperAvailability> {
        let mut child = Command::new(&self.path)
            .arg("availability")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Runtime(format!("helper nativo: {e}")))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stdout.read_to_string(&mut s);
            s
        });
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if start.elapsed() < AVAILABILITY_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Runtime("il helper nativo non risponde".into()));
                }
            }
        }
        let out = reader.join().unwrap_or_default();
        out.lines()
            .find_map(|l| serde_json::from_str::<HelperAvailability>(l).ok())
            .ok_or_else(|| Error::Runtime(format!("risposta del helper non valida: {}", out.trim())))
    }

    /// Runs one generation, forwarding the helper's events. Blocks until it is over. When
    /// `cancel` fires the helper is stopped (its stdin is closed and the process killed) and
    /// the call returns `Ok` without a terminal event.
    pub fn generate(&self, args: &GenerateArgs, cancel: &CancellationToken, on_event: &(dyn Fn(NativeEvent) + Send + Sync)) -> Result<()> {
        let mut child = Command::new(&self.path)
            .arg("generate")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Runtime(format!("helper nativo: {e}")))?;
        // The request is one line; stdin then stays open so that closing it means "stop".
        let mut request = serde_json::json!({
            "system": args.system,
            "prompt": args.prompt,
            "temperature": args.temperature,
            "maxTokens": args.max_tokens,
            "contextTokens": args.context_tokens,
        });
        if !args.tools.is_empty() {
            request["tools"] = serde_json::json!(args.tools);
        }
        if let Some(t) = &args.transcript {
            request["transcript"] = serde_json::json!(t);
        }
        let mut stdin = child.stdin.take().expect("piped stdin");
        if let Err(e) = writeln!(stdin, "{request}").and_then(|_| stdin.flush()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Runtime(format!("helper nativo: {e}")));
        }
        // stderr is drained so that a chatty helper cannot fill the pipe and block.
        let tail = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr_thread = child.stderr.take().map(|mut err| {
            let tail = tail.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = tail.lock().unwrap();
                    t.extend_from_slice(&buf[..n]);
                    if t.len() > STDERR_TAIL {
                        let extra = t.len() - STDERR_TAIL;
                        t.drain(..extra);
                    }
                }
            })
        });
        let stdout = child.stdout.take().expect("piped stdout");

        // Kills the helper as soon as the request is cancelled, which also unblocks the read below.
        let finished = Arc::new(AtomicBool::new(false));
        let shared_child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
        let watcher = {
            let (finished, cancel, shared_child) = (finished.clone(), cancel.clone(), shared_child.clone());
            std::thread::spawn(move || {
                while !finished.load(Ordering::SeqCst) {
                    if cancel.is_cancelled() {
                        if let Some(c) = shared_child.lock().unwrap().as_mut() {
                            let _ = c.kill();
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            })
        };

        let mut terminal = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<NativeEvent>(&line) else { continue };
            if matches!(ev, NativeEvent::Done | NativeEvent::Error { .. }) {
                terminal = true;
            }
            on_event(ev);
            if terminal {
                break;
            }
        }
        finished.store(true, Ordering::SeqCst);
        drop(stdin);
        let _ = watcher.join();
        let status = shared_child.lock().unwrap().take().map(|mut c| {
            // After a terminal event the helper exits by itself (its stdin is closed now);
            // give it a moment, then make sure it does not linger.
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                match c.try_wait() {
                    Ok(Some(s)) => return Ok(s),
                    Ok(None) if Instant::now() < deadline && !cancel.is_cancelled() => std::thread::sleep(Duration::from_millis(10)),
                    _ => {
                        let _ = c.kill();
                        return c.wait();
                    }
                }
            }
        });
        if let Some(t) = stderr_thread {
            let _ = t.join();
        }
        if terminal || cancel.is_cancelled() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&tail.lock().unwrap()).trim().to_string();
        let how = match status {
            Some(Ok(s)) => format!("uscito con {s}"),
            Some(Err(e)) => format!("errore: {e}"),
            None => "interrotto".into(),
        };
        let detail = if stderr.is_empty() { String::new() } else { format!(": {stderr}") };
        Err(Error::Runtime(format!("il helper nativo si è chiuso senza una risposta ({how}){detail}")))
    }
}

/// Writes `bytes` (the helper embedded in the app) into `dir` under a name that carries its
/// content hash, and returns the path. An existing file with the same name is reused: a
/// signed executable is never rewritten in place (macOS kills a process whose file changes).
pub fn extract_embedded(bytes: &[u8], dir: &Path) -> std::io::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let hash: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let target = dir.join(format!("aiw-apple-fm-{hash}"));
    if target.is_file() {
        return Ok(target);
    }
    std::fs::create_dir_all(dir)?;
    // Write elsewhere, make it executable, then move it into place atomically.
    let tmp = dir.join(format!(".aiw-apple-fm-{hash}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

/// Major version of macOS (`26` for macOS 26.x), from `sw_vers`.
#[cfg(target_os = "macos")]
pub fn macos_major() -> Option<u32> {
    let out = Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().split('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Tests that write and run a script one after the other: a parallel test forking in
    /// between would inherit the open file and make `exec` fail with "Text file busy".
    static SERIAL: StdMutex<()> = StdMutex::new(());
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A fake helper: a shell script that follows the protocol.
    #[cfg(unix)]
    fn fake(dir: &Path, body: &str) -> Helper {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-helper");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        Helper::new(p)
    }

    fn collect(h: &Helper, cancel: &CancellationToken) -> (Result<()>, Vec<NativeEvent>) {
        let events = Arc::new(StdMutex::new(Vec::new()));
        let sink = events.clone();
        let r = h.generate(&GenerateArgs { system: "sys".into(), prompt: "ciao".into(), ..Default::default() }, cancel, &move |e| {
            sink.lock().unwrap().push(e)
        });
        let ev = events.lock().unwrap().clone();
        (r, ev)
    }

    #[cfg(unix)]
    #[test]
    fn availability_parses_the_first_json_line() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let ok = fake(dir.path(), r#"echo '{"available": true}'"#);
        assert_eq!(ok.availability().unwrap(), HelperAvailability { available: true, detail: None, context_tokens: None });
        let ctx = fake(dir.path(), r#"echo '{"available": true, "contextTokens": 8192}'"#);
        assert_eq!(ctx.availability().unwrap().context_tokens, Some(8192));
        let no = fake(dir.path(), r#"echo '{"available": false, "detail": "Apple Intelligence è disattivata"}'"#);
        let a = no.availability().unwrap();
        assert!(!a.available && a.detail.unwrap().contains("disattivata"));
        let junk = fake(dir.path(), "echo non json");
        assert!(junk.availability().is_err());
        assert!(Helper::new(dir.path().join("missing")).availability().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn streams_events_until_done() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        // The request is echoed back so the test sees what the helper received.
        let h = fake(
            dir.path(),
            r#"read req
printf '{"type":"text","delta":"Cia"}\n{"type":"text","delta":"o"}\n'
printf '{"type":"text","delta":%s}\n' "$(printf '%s' "$req" | sed 's/"/\\"/g; s/^/"/; s/$/"/')"
printf '{"type":"done"}\n'"#,
        );
        let (r, ev) = collect(&h, &CancellationToken::new());
        r.unwrap();
        assert!(matches!(&ev[0], NativeEvent::Text { delta } if delta == "Cia"));
        assert!(matches!(&ev[1], NativeEvent::Text { delta } if delta == "o"));
        assert!(
            matches!(&ev[2], NativeEvent::Text { delta } if delta.contains("\"prompt\":\"ciao\"") && delta.contains("\"system\":\"sys\"")),
            "{ev:?}"
        );
        assert!(matches!(ev.last(), Some(NativeEvent::Done)));
    }

    #[cfg(unix)]
    #[test]
    fn tools_and_history_reach_the_helper_and_its_tool_calls_come_back() {
        use crate::{NativeCall, NativeTool, NativeTurn};
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        // Answers with a tool call only if the request carries the tools and the history.
        let h = fake(
            dir.path(),
            r#"read req
case "$req" in
  *'"tools":[{'*'"transcript":[{"role":"user","text":"ciao"}]'*)
    printf '{"type":"toolcall","calls":[{"id":"c1","name":"glob","arguments":"{}"}]}\n{"type":"done"}\n' ;;
  *) printf '{"type":"error","message":"richiesta senza strumenti"}
' ;;
esac"#,
        );
        let args = GenerateArgs {
            system: "sys".into(),
            prompt: "elenca".into(),
            tools: vec![NativeTool { name: "glob".into(), description: "Cerca".into(), parameters: serde_json::json!({"type": "object"}) }],
            transcript: Some(vec![NativeTurn::User { text: "ciao".into() }]),
            context_tokens: Some(4096),
            ..Default::default()
        };
        let events = Arc::new(StdMutex::new(Vec::new()));
        let sink = events.clone();
        h.generate(&args, &CancellationToken::new(), &move |e| sink.lock().unwrap().push(e)).unwrap();
        let ev = events.lock().unwrap().clone();
        assert!(
            matches!(&ev[0], NativeEvent::ToolCall { calls } if calls == &vec![NativeCall { id: "c1".into(), name: "glob".into(), arguments: "{}".into() }]),
            "{ev:?}"
        );
        assert!(matches!(ev.last(), Some(NativeEvent::Done)));
    }

    #[cfg(unix)]
    #[test]
    fn helper_error_event_is_forwarded() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let h = fake(dir.path(), r#"read req; printf '{"type":"error","message":"contesto troppo lungo"}\n'; exit 1"#);
        let (r, ev) = collect(&h, &CancellationToken::new());
        r.unwrap();
        assert!(matches!(&ev[0], NativeEvent::Error { message } if message.contains("contesto")));
    }

    #[cfg(unix)]
    #[test]
    fn crash_without_a_reply_is_an_error_with_the_status_and_stderr() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let h = fake(dir.path(), r#"read req; echo "dyld: Library not loaded" 1>&2; exit 7"#);
        let (r, ev) = collect(&h, &CancellationToken::new());
        let err = r.unwrap_err().to_string();
        assert!(ev.is_empty());
        assert!(err.contains("exit status: 7") && err.contains("dyld"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_the_helper() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        // Streams forever, like a long generation.
        let h = fake(dir.path(), r#"read req; while true; do printf '{"type":"text","delta":"x"}\n'; sleep 0.02; done"#);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.cancel();
        });
        let start = Instant::now();
        let (r, ev) = collect(&h, &cancel);
        r.unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!ev.is_empty() && !matches!(ev.last(), Some(NativeEvent::Done)));
    }

    #[test]
    fn extracted_helper_is_named_by_content_and_never_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let a = extract_embedded(b"binary-v1", dir.path()).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"binary-v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let again = extract_embedded(b"binary-v1", dir.path()).unwrap();
        assert_eq!(a, again);
        let b = extract_embedded(b"binary-v2", dir.path()).unwrap();
        assert_ne!(a, b, "a new build gets a new file");
        assert_eq!(std::fs::read(&a).unwrap(), b"binary-v1", "the old one is untouched");
        let leftovers: Vec<_> =
            std::fs::read_dir(dir.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
    }
}
