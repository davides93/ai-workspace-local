//! A backend that falls back to another one when the first fails before producing output.
//!
//! One mechanism for two cases: a platform system model (Apple Foundation Models) that is not
//! ready or errors on the first request continues on a local model, and a run whose runtime was
//! chosen by the router (`runtime_id: "auto"`) moves on to the next candidate when the chosen one
//! fails with a load error. Never once the first event of the stream has been seen: a reply that
//! has started is not restarted on another model.

use crate::types::{ChatBackend, ChatDelta, ChatRequest, DeltaStream};
use aiw_core::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::StreamExt;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Why a fallback is asked for.
#[derive(Debug, Clone)]
pub struct FallbackAttempt {
    /// 1 for the first fallback.
    pub attempt: usize,
    /// The error of the backend that just failed.
    pub error: String,
}

/// Where the request goes instead.
#[derive(Clone)]
pub struct Fallback {
    pub backend: Arc<dyn ChatBackend>,
    /// Model to send to it.
    pub model: String,
    /// Text put in front of the reply to tell the person what happened (`None`: the caller tells
    /// it some other way, e.g. a run event).
    pub prefix: Option<String>,
}

/// Chooses the backend (and the model to send to it) used when the primary one fails; `None` ends
/// the attempts and the original error is returned.
pub type FallbackResolver = Arc<dyn Fn(FallbackAttempt) -> BoxFuture<'static, Option<Fallback>> + Send + Sync>;

pub struct FallbackBackend {
    primary: Arc<dyn ChatBackend>,
    resolver: FallbackResolver,
    max_fallbacks: usize,
}

impl FallbackBackend {
    pub fn new(primary: Arc<dyn ChatBackend>, resolver: FallbackResolver) -> Self {
        Self { primary, resolver, max_fallbacks: 1 }
    }

    /// Most fallbacks for one request (default 1).
    pub fn with_max_fallbacks(mut self, n: usize) -> Self {
        self.max_fallbacks = n;
        self
    }
}

/// The stream of `backend` once its first event has arrived, or the error that came instead.
async fn first_event(
    backend: &Arc<dyn ChatBackend>,
    req: ChatRequest,
    cancel: CancellationToken,
) -> std::result::Result<Option<DeltaStream>, aiw_core::Error> {
    let mut stream = backend.chat(req, cancel).await?;
    match stream.next().await {
        Some(Ok(first)) => Ok(Some(futures::stream::once(async move { Ok(first) }).chain(stream).boxed())),
        Some(Err(e)) => Err(e),
        // Closed without a single event: nothing to show, let the caller see it.
        None => Ok(None),
    }
}

#[async_trait]
impl ChatBackend for FallbackBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        self.primary.list_models().await
    }

    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.primary.embed(model, texts).await
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let mut backend = self.primary.clone();
        let mut current = req;
        let mut prefix: Option<String> = None;
        let mut first_error: Option<aiw_core::Error> = None;
        for attempt in 0..=self.max_fallbacks {
            let error = match first_event(&backend, current.clone(), cancel.clone()).await {
                Ok(Some(stream)) => {
                    return Ok(match prefix {
                        Some(note) => futures::stream::once(async move { Ok(ChatDelta::Text(note)) }).chain(stream).boxed(),
                        None => stream,
                    });
                }
                Ok(None) => return Ok(futures::stream::empty().boxed()),
                Err(e) => e,
            };
            if cancel.is_cancelled() {
                return Err(first_error.unwrap_or(error));
            }
            let message = error.to_string();
            let first = first_error.get_or_insert(error);
            if attempt == self.max_fallbacks {
                break;
            }
            let Some(next) = (self.resolver)(FallbackAttempt { attempt: attempt + 1, error: message.clone() }).await else { break };
            tracing::warn!("backend failed ({message}); continuing on model {}", next.model);
            let _ = first;
            current.model = next.model;
            backend = next.backend;
            prefix = next.prefix;
        }
        Err(first_error.unwrap_or_else(|| aiw_core::Error::Runtime("nessun backend disponibile".into())))
    }

    fn availability(&self) -> std::result::Result<(), String> {
        self.primary.availability()
    }

    fn served_context(&self) -> Option<u32> {
        self.primary.served_context()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripted::ScriptedBackend;

    struct Failing;
    #[async_trait]
    impl ChatBackend for Failing {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
        async fn chat(&self, _: ChatRequest, _: CancellationToken) -> Result<DeltaStream> {
            Err(aiw_core::Error::Runtime("boom".into()))
        }
    }

    async fn collect(b: &FallbackBackend) -> Result<String> {
        let mut s = b.chat(ChatRequest::default(), CancellationToken::new()).await?;
        let mut out = String::new();
        while let Some(d) = s.next().await {
            if let ChatDelta::Text(t) = d? {
                out.push_str(&t);
            }
        }
        Ok(out)
    }

    fn resolver(text: &'static str) -> FallbackResolver {
        Arc::new(move |attempt| {
            Box::pin(async move {
                let b: Arc<dyn ChatBackend> = Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn(text)]));
                Some(Fallback {
                    backend: b,
                    model: "local".to_string(),
                    prefix: Some(format!("[Modello di sistema non disponibile ({}): risposta di un modello locale]\n\n", attempt.error)),
                })
            })
        })
    }

    #[tokio::test]
    async fn falls_back_when_primary_fails() {
        let b = FallbackBackend::new(Arc::new(Failing), resolver("ciao"));
        let out = collect(&b).await.unwrap();
        assert!(out.contains("ciao") && out.contains("boom"));
    }

    #[tokio::test]
    async fn keeps_primary_when_it_works() {
        let p: Arc<dyn ChatBackend> = Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn("primo")]));
        let b = FallbackBackend::new(p, resolver("secondo"));
        assert_eq!(collect(&b).await.unwrap(), "primo");
    }

    #[tokio::test]
    async fn reports_original_error_without_fallback() {
        let r: FallbackResolver = Arc::new(|_| Box::pin(async { None }));
        let b = FallbackBackend::new(Arc::new(Failing), r);
        assert!(collect(&b).await.is_err());
    }
}
