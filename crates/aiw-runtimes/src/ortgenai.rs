//! ONNX Runtime GenAI, loaded at run time through its stable C API (`ort_genai_c.h`).
//!
//! Nothing is linked at build time: the app works without it and picks the library up once the
//! user installs it (the `onnxruntime-genai` package for DirectML, CUDA, QNN, OpenVINO, CoreML or
//! CPU). Models are ONNX GenAI folders (with `genai_config.json`). Tool use follows the text
//! protocol shared with the embedded llama.cpp engine.

use crate::registry::ModelResolver;
use crate::textproto;
use crate::types::*;
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::StreamExt;
use libloading::Library;
use serde_json::json;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// `OgaResult*`: null on success.
type R = *mut c_void;

macro_rules! api {
    ($($field:ident = $sym:literal : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        struct Api {
            _lib: Library,
            $($field: unsafe extern "system" fn($($arg),*) $(-> $ret)?,)*
        }
        impl Api {
            fn load(lib: Library) -> std::result::Result<Self, String> {
                unsafe {
                    $(let $field = *lib
                        .get::<unsafe extern "system" fn($($arg),*) $(-> $ret)?>(concat!($sym, "\0").as_bytes())
                        .map_err(|e| format!("simbolo {} mancante ({e}): versione di onnxruntime-genai troppo vecchia?", $sym))?;)*
                    Ok(Api { _lib: lib, $($field,)* })
                }
            }
        }
    };
}

api! {
    result_get_error = "OgaResultGetError": fn(*const c_void) -> *const c_char;
    destroy_result = "OgaDestroyResult": fn(*mut c_void);
    destroy_string = "OgaDestroyString": fn(*const c_char);
    create_config = "OgaCreateConfig": fn(*const c_char, *mut *mut c_void) -> R;
    config_clear_providers = "OgaConfigClearProviders": fn(*mut c_void) -> R;
    config_append_provider = "OgaConfigAppendProvider": fn(*mut c_void, *const c_char) -> R;
    destroy_config = "OgaDestroyConfig": fn(*mut c_void);
    create_model_from_config = "OgaCreateModelFromConfig": fn(*const c_void, *mut *mut c_void) -> R;
    destroy_model = "OgaDestroyModel": fn(*mut c_void);
    create_tokenizer = "OgaCreateTokenizer": fn(*const c_void, *mut *mut c_void) -> R;
    destroy_tokenizer = "OgaDestroyTokenizer": fn(*mut c_void);
    apply_chat_template = "OgaTokenizerApplyChatTemplate": fn(*const c_void, *const c_char, *const c_char, *const c_char, bool, *mut *const c_char) -> R;
    create_sequences = "OgaCreateSequences": fn(*mut *mut c_void) -> R;
    destroy_sequences = "OgaDestroySequences": fn(*mut c_void);
    sequence_len = "OgaSequencesGetSequenceCount": fn(*const c_void, usize) -> usize;
    tokenizer_encode = "OgaTokenizerEncode": fn(*const c_void, *const c_char, *mut c_void) -> R;
    create_params = "OgaCreateGeneratorParams": fn(*const c_void, *mut *mut c_void) -> R;
    destroy_params = "OgaDestroyGeneratorParams": fn(*mut c_void);
    set_number = "OgaGeneratorParamsSetSearchNumber": fn(*mut c_void, *const c_char, f64) -> R;
    set_bool = "OgaGeneratorParamsSetSearchBool": fn(*mut c_void, *const c_char, bool) -> R;
    create_generator = "OgaCreateGenerator": fn(*const c_void, *const c_void, *mut *mut c_void) -> R;
    destroy_generator = "OgaDestroyGenerator": fn(*mut c_void);
    append_sequences = "OgaGenerator_AppendTokenSequences": fn(*mut c_void, *const c_void) -> R;
    is_done = "OgaGenerator_IsDone": fn(*mut c_void) -> bool;
    generate_next = "OgaGenerator_GenerateNextToken": fn(*mut c_void) -> R;
    next_tokens = "OgaGenerator_GetNextTokens": fn(*const c_void, *mut *const i32, *mut usize) -> R;
    create_stream = "OgaCreateTokenizerStream": fn(*const c_void, *mut *mut c_void) -> R;
    destroy_stream = "OgaDestroyTokenizerStream": fn(*mut c_void);
    stream_decode = "OgaTokenizerStreamDecode": fn(*mut c_void, i32, *mut *const c_char) -> R;
}

impl Api {
    /// Turns an `OgaResult*` into an error (and frees it).
    fn ok(&self, r: R, what: &str) -> Result<()> {
        if r.is_null() {
            return Ok(());
        }
        let msg = unsafe {
            let p = (self.result_get_error)(r);
            let m = if p.is_null() { "errore sconosciuto".to_string() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
            (self.destroy_result)(r);
            m
        };
        Err(Error::Runtime(format!("onnxruntime-genai, {what}: {msg}")))
    }
}

/// Owned GenAI object, destroyed on drop.
struct Obj<'a> {
    ptr: *mut c_void,
    drop: unsafe extern "system" fn(*mut c_void),
    _api: std::marker::PhantomData<&'a Api>,
}
impl Drop for Obj<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { (self.drop)(self.ptr) };
        }
    }
}

fn obj<'a>(_api: &'a Api, ptr: *mut c_void, drop: unsafe extern "system" fn(*mut c_void)) -> Obj<'a> {
    Obj { ptr, drop, _api: std::marker::PhantomData }
}

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Invalid("testo con un carattere NUL".into()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrtGenAiSettings {
    /// File or folder of the onnxruntime-genai library (empty = search next to the app and in
    /// the system library path; `ORT_GENAI_LIB` also works).
    pub library_path: String,
    /// auto (as in genai_config.json) | cpu | cuda | dml | qnn | openvino | coreml | webgpu
    pub execution_provider: String,
    /// Upper bound for prompt + answer tokens (`max_length`).
    pub max_length: u32,
}

impl Default for OrtGenAiSettings {
    fn default() -> Self {
        Self { library_path: String::new(), execution_provider: "auto".into(), max_length: 4096 }
    }
}

const LIB_NAME: &str = if cfg!(windows) {
    "onnxruntime-genai.dll"
} else if cfg!(target_os = "macos") {
    "libonnxruntime-genai.dylib"
} else {
    "libonnxruntime-genai.so"
};

fn ort_core_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["onnxruntime.dll"]
    } else if cfg!(target_os = "macos") {
        &["libonnxruntime.dylib"]
    } else {
        &["libonnxruntime.so.1", "libonnxruntime.so"]
    }
}

fn candidates(setting: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut push = |p: PathBuf| v.push(if p.is_dir() { p.join(LIB_NAME) } else { p });
    if !setting.trim().is_empty() {
        push(PathBuf::from(setting.trim()));
    }
    if let Some(p) = std::env::var_os("ORT_GENAI_LIB") {
        push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        push(dir.join(LIB_NAME));
    }
    v.push(PathBuf::from(LIB_NAME)); // system search path
    v
}

/// Loads onnxruntime-genai. The ONNX Runtime core next to it is loaded first, so that the
/// library's own lookup by name finds it already in memory.
fn load_api(setting: &str) -> std::result::Result<Api, String> {
    let mut tried = Vec::new();
    for path in candidates(setting) {
        if path.is_absolute() && !path.exists() {
            tried.push(path.display().to_string());
            continue;
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            for name in ort_core_names() {
                let core = dir.join(name);
                if core.exists() {
                    // Kept loaded for the life of the process, like the library itself.
                    if let Ok(lib) = unsafe { Library::new(&core) } {
                        std::mem::forget(lib);
                    }
                    break;
                }
            }
        }
        match unsafe { Library::new(&path) } {
            Ok(lib) => return Api::load(lib),
            Err(e) => tried.push(format!("{} ({e})", path.display())),
        }
    }
    Err(format!(
        "libreria onnxruntime-genai non trovata. Installala (pip install onnxruntime-genai, o il pacchetto NuGet/archivio della release) e indica il percorso nelle impostazioni del runtime. Provati: {}",
        tried.join("; ")
    ))
}

struct Loaded {
    key: (PathBuf, String),
    model: *mut c_void,
    tokenizer: *mut c_void,
}
// The GenAI model and tokenizer are thread-safe objects; access is serialised by `Inner::gen`.
unsafe impl Send for Loaded {}

struct Inner {
    resolver: ModelResolver,
    settings: Mutex<OrtGenAiSettings>,
    api: Mutex<Option<Arc<Api>>>,
    loaded: Mutex<Option<Loaded>>,
}

pub struct OrtGenAi {
    inner: Arc<Inner>,
}

impl OrtGenAi {
    pub fn new(resolver: ModelResolver, settings: OrtGenAiSettings) -> Self {
        Self { inner: Arc::new(Inner { resolver, settings: Mutex::new(settings), api: Mutex::new(None), loaded: Mutex::new(None) }) }
    }
    pub fn set_settings(&self, s: OrtGenAiSettings) {
        *self.inner.settings.lock().unwrap() = s;
    }
    /// Loads the library now (to report availability); later calls reuse it.
    pub fn probe(&self) -> Result<()> {
        self.inner.api().map(|_| ())
    }
}

impl Inner {
    fn api(&self) -> Result<Arc<Api>> {
        let mut slot = self.api.lock().unwrap();
        if let Some(a) = slot.as_ref() {
            return Ok(a.clone());
        }
        let path = self.settings.lock().unwrap().library_path.clone();
        let api = Arc::new(load_api(&path).map_err(Error::Unsupported)?);
        *slot = Some(api.clone());
        Ok(api)
    }

    fn unload(api: &Api, l: Loaded) {
        unsafe {
            (api.destroy_tokenizer)(l.tokenizer);
            (api.destroy_model)(l.model);
        }
    }

    /// Makes sure the requested model folder is resident (one at a time).
    fn ensure_model(&self, api: &Api, model_id: &str, provider: &str) -> Result<()> {
        let path = (self.resolver)(model_id)
            .ok_or_else(|| Error::NotFound(format!("modello ONNX `{model_id}`: indica la cartella con genai_config.json")))?;
        let dir = if path.is_file() { path.parent().map(Path::to_path_buf).unwrap_or(path) } else { path };
        if !dir.join("genai_config.json").exists() {
            return Err(Error::Invalid(format!("{} non contiene genai_config.json (modello ONNX GenAI)", dir.display())));
        }
        let key = (dir.clone(), provider.to_string());
        let mut slot = self.loaded.lock().unwrap();
        if slot.as_ref().map(|l| l.key == key).unwrap_or(false) {
            return Ok(());
        }
        if let Some(old) = slot.take() {
            Self::unload(api, old);
        }
        unsafe {
            let dir_c = cstr(&dir.to_string_lossy())?;
            let mut config = null_mut();
            api.ok((api.create_config)(dir_c.as_ptr(), &mut config), "configurazione")?;
            let config = obj(api, config, api.destroy_config);
            if provider != "auto" && !provider.is_empty() {
                api.ok((api.config_clear_providers)(config.ptr), "provider")?;
                if provider != "cpu" {
                    let p = cstr(provider)?;
                    api.ok((api.config_append_provider)(config.ptr, p.as_ptr()), "provider")?;
                }
            }
            let mut model = null_mut();
            api.ok((api.create_model_from_config)(config.ptr, &mut model), "caricamento del modello")?;
            let mut tokenizer = null_mut();
            if let Err(e) = api.ok((api.create_tokenizer)(model, &mut tokenizer), "tokenizer") {
                (api.destroy_model)(model);
                return Err(e);
            }
            *slot = Some(Loaded { key, model, tokenizer });
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let (Some(api), Some(l)) = (self.api.get_mut().unwrap().clone(), self.loaded.get_mut().unwrap().take()) {
            Self::unload(&api, l);
        }
    }
}

/// Emits the longest valid UTF-8 prefix of `pending`, keeping an incomplete tail.
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
                pending.clear();
            }
            out
        }
    }
}

fn generate(
    inner: &Inner,
    api: &Api,
    req: ChatRequest,
    tx: tokio::sync::mpsc::UnboundedSender<Result<ChatDelta>>,
    cancel: CancellationToken,
) -> Result<()> {
    let settings = inner.settings.lock().unwrap().clone();
    inner.ensure_model(api, &req.model, &settings.execution_provider)?;
    let slot = inner.loaded.lock().unwrap(); // one generation at a time on the resident model
    let l = slot.as_ref().ok_or_else(|| Error::Runtime("modello non caricato".into()))?;
    let turns = textproto::chat_turns(&req);
    let messages: Vec<serde_json::Value> = turns.iter().map(|(r, c)| json!({"role": r, "content": c})).collect();
    unsafe {
        // Prompt: the model's chat template, ChatML when it has none.
        let messages_c = cstr(&serde_json::to_string(&messages)?)?;
        let mut out: *const c_char = null();
        let templated = (api.apply_chat_template)(l.tokenizer, null(), messages_c.as_ptr(), null(), true, &mut out);
        let prompt = if templated.is_null() && !out.is_null() {
            let s = CStr::from_ptr(out).to_string_lossy().into_owned();
            (api.destroy_string)(out);
            s
        } else {
            if !templated.is_null() {
                (api.destroy_result)(templated);
            }
            textproto::chatml(&turns)
        };
        let mut seqs = null_mut();
        api.ok((api.create_sequences)(&mut seqs), "sequenze")?;
        let seqs = obj(api, seqs, api.destroy_sequences);
        let prompt_c = cstr(&prompt)?;
        api.ok((api.tokenizer_encode)(l.tokenizer, prompt_c.as_ptr(), seqs.ptr), "tokenizzazione")?;
        let n_prompt = (api.sequence_len)(seqs.ptr, 0);
        let max_length = settings.max_length.max(256) as usize;
        if n_prompt + 8 >= max_length {
            return Err(Error::Invalid(format!(
                "prompt di {n_prompt} token oltre il limite di {max_length}: aumentalo nelle impostazioni o compatta"
            )));
        }
        let max_new = req.max_tokens.unwrap_or(2048) as usize;
        let mut params = null_mut();
        api.ok((api.create_params)(l.model, &mut params), "parametri")?;
        let params = obj(api, params, api.destroy_params);
        let set_n = |k: &str, v: f64| -> Result<()> {
            let k = cstr(k)?;
            api.ok((api.set_number)(params.ptr, k.as_ptr(), v), "parametri")
        };
        set_n("max_length", (n_prompt + max_new).min(max_length) as f64)?;
        let temp = req.temperature.unwrap_or(0.7);
        let sample = cstr("do_sample")?;
        api.ok((api.set_bool)(params.ptr, sample.as_ptr(), temp > 0.0), "parametri")?;
        if temp > 0.0 {
            set_n("temperature", temp)?;
            set_n("top_p", req.top_p.unwrap_or(0.95))?;
            set_n("top_k", 40.0)?;
        }
        let mut generator = null_mut();
        api.ok((api.create_generator)(l.model, params.ptr, &mut generator), "generatore")?;
        let generator = obj(api, generator, api.destroy_generator);
        api.ok((api.append_sequences)(generator.ptr, seqs.ptr), "prompt")?;
        let mut stream = null_mut();
        api.ok((api.create_stream)(l.tokenizer, &mut stream), "decodifica")?;
        let stream = obj(api, stream, api.destroy_stream);

        let started = Instant::now();
        let mut produced = 0usize;
        let mut text = String::new();
        let mut pending: Vec<u8> = Vec::new();
        let mut finish = "stop";
        loop {
            if cancel.is_cancelled() {
                finish = "cancelled";
                break;
            }
            if (api.is_done)(generator.ptr) {
                if produced >= max_new {
                    finish = "length";
                }
                break;
            }
            api.ok((api.generate_next)(generator.ptr), "generazione")?;
            let (mut toks, mut count): (*const i32, usize) = (null(), 0);
            api.ok((api.next_tokens)(generator.ptr, &mut toks, &mut count), "token")?;
            if toks.is_null() || count == 0 {
                break;
            }
            let token = *toks;
            produced += 1;
            let mut piece: *const c_char = null();
            api.ok((api.stream_decode)(stream.ptr, token, &mut piece), "decodifica")?;
            if !piece.is_null() {
                pending.extend_from_slice(CStr::from_ptr(piece).to_bytes());
            }
            let piece = take_utf8(&mut pending);
            if !piece.is_empty() {
                text.push_str(&piece);
                if req.stop.iter().any(|s| !s.is_empty() && text.ends_with(s.as_str())) {
                    break;
                }
                if tx.send(Ok(ChatDelta::Text(piece))).is_err() {
                    return Ok(());
                }
            }
        }
        let secs = started.elapsed().as_secs_f64().max(1e-6);
        let _ = tx.send(Ok(ChatDelta::Usage(TokenUsage {
            prompt_tokens: n_prompt as u64,
            completion_tokens: produced as u64,
            tokens_per_s: Some(produced as f64 / secs),
        })));
        let _ = tx.send(Ok(ChatDelta::Done { finish_reason: Some(finish.into()) }));
    }
    Ok(())
}

#[async_trait]
impl ChatBackend for OrtGenAi {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let inner = self.inner.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let tx2 = tx.clone();
        std::thread::Builder::new()
            .name("aiw-ort-genai".into())
            .spawn(move || {
                let result = inner.api().and_then(|api| generate(&inner, &api, req, tx2.clone(), cancel));
                if let Err(e) = result {
                    let _ = tx2.send(Err(e));
                }
            })
            .map_err(|e| Error::Runtime(e.to_string()))?;
        drop(tx);
        let mut rx = rx;
        Ok(futures::stream::poll_fn(move |cx| rx.poll_recv(cx)).boxed())
    }

    fn availability(&self) -> std::result::Result<(), String> {
        self.inner.api().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_is_reported_with_the_paths_tried() {
        let e = load_api("/nessun/percorso/libonnxruntime-genai.so").err().unwrap();
        assert!(e.contains("non trovata") && e.contains("/nessun/percorso"), "{e}");
        let engine = OrtGenAi::new(Arc::new(|_| None), OrtGenAiSettings { library_path: "/nessun/percorso".into(), ..Default::default() });
        if std::env::var_os("ORT_GENAI_LIB").is_none() {
            assert!(engine.availability().is_err());
        }
    }

    /// Real run: `AIW_TEST_ORT_GENAI_LIB=/dir/with/lib AIW_TEST_ORT_GENAI_MODEL=/model/dir cargo test -p aiw-runtimes ortgenai -- --ignored`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn generates_with_a_real_model() {
        let (Ok(lib), Ok(model)) = (std::env::var("AIW_TEST_ORT_GENAI_LIB"), std::env::var("AIW_TEST_ORT_GENAI_MODEL")) else { return };
        let dir = PathBuf::from(model);
        let engine = OrtGenAi::new(Arc::new(move |_| Some(dir.clone())), OrtGenAiSettings { library_path: lib, ..Default::default() });
        engine.probe().unwrap();
        let req = ChatRequest {
            model: "test".into(),
            messages: vec![Msg::system("You are a helpful assistant. Answer with one word."), Msg::user("What is the capital of France?")],
            temperature: Some(0.0),
            max_tokens: Some(32),
            ..Default::default()
        };
        let mut s = engine.chat(req, CancellationToken::new()).await.unwrap();
        let (mut text, mut usage) = (String::new(), None);
        while let Some(d) = s.next().await {
            match d.unwrap() {
                ChatDelta::Text(t) => text.push_str(&t),
                ChatDelta::Usage(u) => usage = Some(u),
                _ => {}
            }
        }
        println!("risposta: {text:?} {usage:?}");
        // Tiny int4 models may get facts wrong: the engine is what is under test.
        assert!(!text.trim().is_empty(), "nessun testo generato");
        assert!(usage.unwrap().completion_tokens > 0);
    }
}
