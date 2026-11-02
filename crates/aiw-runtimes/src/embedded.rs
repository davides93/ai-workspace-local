//! llama.cpp linked in-process (feature `llama-embedded`): no sidecar, no ports. This is the
//! default engine on Android and iOS, and an alternative to `llama-server` on desktop.
//!
//! Tool use goes through the text protocol (`<tool_call>{…}</tool_call>`): the built-in
//! llama.cpp template engine does not render OpenAI tool schemas, so tools are described in the
//! system message and the agent loop parses the calls.

use crate::registry::ModelResolver;
use crate::types::*;
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::StreamExt;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
static BACKEND_INIT: Mutex<()> = Mutex::new(());

fn backend() -> Result<&'static LlamaBackend> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let _guard = BACKEND_INIT.lock().unwrap();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let mut b = LlamaBackend::init().map_err(|e| Error::Runtime(format!("llama.cpp: {e}")))?;
    b.void_logs();
    let _ = BACKEND.set(b);
    Ok(BACKEND.get().expect("backend set"))
}

#[derive(Debug, Clone)]
pub struct EmbeddedSettings {
    pub ctx_size: u32,
    pub gpu_layers: u32,
    pub threads: Option<i32>,
}

impl Default for EmbeddedSettings {
    fn default() -> Self {
        Self { ctx_size: 8192, gpu_layers: 999, threads: None }
    }
}

pub struct EmbeddedLlama {
    inner: Arc<Inner>,
}

struct Inner {
    resolver: ModelResolver,
    settings: Mutex<EmbeddedSettings>,
    models: Mutex<HashMap<PathBuf, Arc<LlamaModel>>>,
}

impl EmbeddedLlama {
    pub fn new(resolver: ModelResolver, settings: EmbeddedSettings) -> Self {
        Self { inner: Arc::new(Inner { resolver, settings: Mutex::new(settings), models: Mutex::new(HashMap::new()) }) }
    }

    pub fn set_settings(&self, s: EmbeddedSettings) {
        *self.inner.settings.lock().unwrap() = s;
    }
}

impl Inner {
    fn load(&self, model_id: &str) -> Result<Arc<LlamaModel>> {
        let path = (self.resolver)(model_id).ok_or_else(|| Error::NotFound(format!("modello `{model_id}`: scaricalo da Modelli")))?;
        if let Some(m) = self.models.lock().unwrap().get(&path) {
            return Ok(m.clone());
        }
        let gpu_layers = self.settings.lock().unwrap().gpu_layers;
        let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let model = LlamaModel::load_from_file(backend()?, &path, &params)
            .map_err(|e| Error::Runtime(format!("caricamento di {}: {e}", path.display())))?;
        let model = Arc::new(model);
        let mut cache = self.models.lock().unwrap();
        cache.clear(); // keep one model resident: mobile memory is precious
        cache.insert(path, model.clone());
        Ok(model)
    }
}

/// Renders the conversation with the model's own chat template (falls back to ChatML).
pub fn render_prompt(model: &LlamaModel, req: &ChatRequest) -> Result<String> {
    let mut msgs: Vec<(String, String)> = Vec::new();
    let tools = if req.tools.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = req
            .tools
            .iter()
            .map(|t| format!("- {}: {} (argomenti: {})", t.function.name, t.function.description, t.function.parameters))
            .collect();
        format!(
            "\n\n# Strumenti\nPer usare uno strumento rispondi SOLO con un blocco <tool_call>{{\"name\": \"nome\", \"arguments\": {{...}}}}</tool_call>.\n{}",
            list.join("\n")
        )
    };
    let mut system_done = false;
    for m in &req.messages {
        let text = m.text();
        match m.role.as_str() {
            "system" => {
                msgs.push(("system".into(), format!("{text}{tools}")));
                system_done = true;
            }
            "assistant" => {
                let calls = m
                    .tool_calls
                    .as_ref()
                    .map(|c| {
                        c.iter()
                            .map(|t| {
                                format!(
                                    "<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>",
                                    t.function.name, t.function.arguments
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                msgs.push(("assistant".into(), format!("{text}{calls}")));
            }
            "tool" => msgs.push((
                "user".into(),
                format!("<tool_response name=\"{}\">\n{text}\n</tool_response>", m.name.clone().unwrap_or_default()),
            )),
            _ => msgs.push(("user".into(), text)),
        }
    }
    if !system_done && !tools.is_empty() {
        msgs.insert(0, ("system".into(), tools.trim_start().to_string()));
    }
    let chat: Vec<LlamaChatMessage> = msgs
        .into_iter()
        .map(|(r, c)| LlamaChatMessage::new(r, c))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let template = model
        .chat_template(None)
        .or_else(|_| LlamaChatTemplate::new("chatml").map_err(|e| Error::Runtime(e.to_string())))
        .map_err(|e| Error::Runtime(format!("{e}")))?;
    model.apply_chat_template(&template, &chat, true).map_err(|e| Error::Runtime(format!("chat template: {e}")))
}

/// Emits the longest valid UTF-8 prefix of `pending`, keeping an incomplete tail for later.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(s) => {
            let out = s.to_string();
            pending.clear();
            out
        }
        Err(e) => {
            let valid = e.valid_up_to();
            let out = String::from_utf8_lossy(&pending[..valid]).into_owned();
            pending.drain(..valid);
            if e.error_len().is_some() {
                pending.clear(); // invalid bytes, not just an unfinished sequence
            }
            out
        }
    }
}

fn generate(
    model: Arc<LlamaModel>,
    settings: EmbeddedSettings,
    req: ChatRequest,
    tx: tokio::sync::mpsc::UnboundedSender<Result<ChatDelta>>,
    cancel: CancellationToken,
) -> Result<()> {
    let backend = backend()?;
    let prompt = render_prompt(&model, &req)?;
    let vocab = model.vocab();
    let bos_text = String::from_utf8_lossy(&vocab.token_to_piece(vocab.bos(), true, None)).into_owned();
    let add_bos = !bos_text.is_empty() && !prompt.starts_with(&bos_text);
    let tokens = vocab.tokenize(prompt.as_bytes(), add_bos, true);
    let n_ctx = settings.ctx_size.max(512);
    if tokens.len() as u32 + 16 >= n_ctx {
        return Err(Error::Invalid(format!(
            "prompt di {} token oltre il contesto di {n_ctx}: aumenta la finestra o compatta",
            tokens.len()
        )));
    }
    let n_batch: usize = 512;
    let mut ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(n_ctx)).with_n_batch(n_batch as u32);
    let threads = settings.threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4));
    ctx_params = ctx_params.with_n_threads(threads).with_n_threads_batch(threads);
    let mut ctx = model.new_context(backend, ctx_params).map_err(|e| Error::Runtime(format!("contesto: {e}")))?;
    let mut batch = LlamaBatch::new(n_batch, 1);
    let n_prompt = tokens.len();
    for (ci, chunk) in tokens.chunks(n_batch).enumerate() {
        batch.clear();
        for (j, t) in chunk.iter().enumerate() {
            let pos = ci * n_batch + j;
            batch.add(*t, pos as i32, &[0], pos + 1 == n_prompt).map_err(|e| Error::Runtime(e.to_string()))?;
        }
        ctx.decode(&mut batch).map_err(|e| Error::Runtime(format!("decode del prompt: {e}")))?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
    }
    let temp = req.temperature.unwrap_or(0.7) as f32;
    let mut sampler = if temp <= 0.0 {
        LlamaSampler::greedy()
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(req.top_p.unwrap_or(0.95) as f32, 1),
            LlamaSampler::temp(temp),
            LlamaSampler::dist(rand_seed()),
        ])
    };
    let max_new = req.max_tokens.unwrap_or(2048).min(n_ctx.saturating_sub(n_prompt as u32 + 1));
    let started = Instant::now();
    let mut n_cur = n_prompt;
    let mut produced = 0u32;
    let mut pending: Vec<u8> = Vec::new();
    let mut text = String::new();
    let mut finish = "length";
    while produced < max_new {
        if cancel.is_cancelled() {
            finish = "cancelled";
            break;
        }
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);
        if vocab.is_eog(token) {
            finish = "stop";
            break;
        }
        pending.extend(vocab.token_to_piece(token, false, None));
        let piece = take_utf8(&mut pending);
        produced += 1;
        if !piece.is_empty() {
            text.push_str(&piece);
            if let Some(stop) = req.stop.iter().find(|s| !s.is_empty() && text.ends_with(s.as_str())) {
                let _ = stop;
                finish = "stop";
                break;
            }
            if tx.send(Ok(ChatDelta::Text(piece))).is_err() {
                return Ok(()); // receiver dropped
            }
        }
        batch.clear();
        batch.add(token, n_cur as i32, &[0], true).map_err(|e| Error::Runtime(e.to_string()))?;
        n_cur += 1;
        ctx.decode(&mut batch).map_err(|e| Error::Runtime(format!("decode: {e}")))?;
    }
    let secs = started.elapsed().as_secs_f64().max(1e-6);
    let _ = tx.send(Ok(ChatDelta::Usage(TokenUsage {
        prompt_tokens: n_prompt as u64,
        completion_tokens: produced as u64,
        tokens_per_s: Some(produced as f64 / secs),
    })));
    let _ = tx.send(Ok(ChatDelta::Done { finish_reason: Some(finish.into()) }));
    Ok(())
}

fn rand_seed() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(42)
}

#[async_trait]
impl ChatBackend for EmbeddedLlama {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let inner = self.inner.clone();
        let settings = inner.settings.lock().unwrap().clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let tx2 = tx.clone();
        // Model loading and generation run on a dedicated thread (blocking FFI).
        std::thread::Builder::new()
            .name("aiw-llama".into())
            .spawn(move || {
                let result = inner.load(&req.model).and_then(|model| generate(model, settings, req, tx2.clone(), cancel));
                if let Err(e) = result {
                    let _ = tx2.send(Err(e));
                }
            })
            .map_err(|e| Error::Runtime(e.to_string()))?;
        drop(tx);
        let mut rx = rx;
        Ok(futures::stream::poll_fn(move |cx| rx.poll_recv(cx)).boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_pieces_are_reassembled() {
        let mut p = "è".as_bytes()[..1].to_vec();
        assert_eq!(take_utf8(&mut p), "");
        p.extend(&"è".as_bytes()[1..]);
        p.extend(b"!");
        assert_eq!(take_utf8(&mut p), "è!");
        assert!(p.is_empty());
    }

    /// Real model test: `AIW_TEST_GGUF=/path/model.gguf cargo test -p aiw-runtimes --features llama-embedded -- --ignored`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn generates_with_a_real_gguf() {
        let Ok(path) = std::env::var("AIW_TEST_GGUF") else { return };
        let p = PathBuf::from(path);
        let engine =
            EmbeddedLlama::new(Arc::new(move |_| Some(p.clone())), EmbeddedSettings { ctx_size: 4096, gpu_layers: 0, threads: None });
        let req = ChatRequest {
            model: "test".into(),
            messages: vec![Msg::system("Rispondi in italiano, in una parola."), Msg::user("Qual è la capitale della Francia? /no_think")],
            temperature: Some(0.0),
            max_tokens: Some(64),
            ..Default::default()
        };
        let mut s = engine.chat(req, CancellationToken::new()).await.unwrap();
        let mut text = String::new();
        let mut usage = None;
        while let Some(d) = s.next().await {
            match d.unwrap() {
                ChatDelta::Text(t) => text.push_str(&t),
                ChatDelta::Usage(u) => usage = Some(u),
                _ => {}
            }
        }
        println!("output: {text:?} usage: {usage:?}");
        assert!(text.to_lowercase().contains("parigi"), "{text}");
        assert!(usage.unwrap().completion_tokens > 0);
    }
}
