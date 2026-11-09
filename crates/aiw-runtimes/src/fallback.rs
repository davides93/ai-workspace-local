//! A backend that falls back to another one when the first fails before producing output.
//!
//! Used for platform system models (Apple Foundation Models): when the system model is not
//! ready, refuses to start or errors on the first request, the conversation continues on a
//! local model instead of ending with an error.

use crate::types::{ChatBackend, ChatDelta, ChatRequest, DeltaStream};
use aiw_core::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::StreamExt;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Chooses the backend (and the model to send to it) used when the primary one fails.
pub type FallbackResolver = Arc<dyn Fn() -> BoxFuture<'static, Option<(Arc<dyn ChatBackend>, String)>> + Send + Sync>;

pub struct FallbackBackend {
    primary: Arc<dyn ChatBackend>,
    resolver: FallbackResolver,
}

impl FallbackBackend {
    pub fn new(primary: Arc<dyn ChatBackend>, resolver: FallbackResolver) -> Self {
        Self { primary, resolver }
    }
}

#[async_trait]
impl ChatBackend for FallbackBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        self.primary.list_models().await
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let first_error = match self.primary.chat(req.clone(), cancel.clone()).await {
            Ok(mut stream) => match stream.next().await {
                Some(Ok(first)) => {
                    return Ok(futures::stream::once(async move { Ok(first) }).chain(stream).boxed());
                }
                Some(Err(e)) => e,
                // Closed without a single event: nothing to show, let the caller see it.
                None => return Ok(futures::stream::empty().boxed()),
            },
            Err(e) => e,
        };
        if cancel.is_cancelled() {
            return Err(first_error);
        }
        let Some((backend, model)) = (self.resolver)().await else { return Err(first_error) };
        tracing::warn!("system model failed ({first_error}); continuing on the local model {model}");
        let mut next = req;
        next.model = model;
        match backend.chat(next, cancel).await {
            Ok(stream) => {
                let note = format!("[Modello di sistema non disponibile ({first_error}): risposta di un modello locale]\n\n");
                Ok(futures::stream::once(async move { Ok(ChatDelta::Text(note)) }).chain(stream).boxed())
            }
            Err(_) => Err(first_error),
        }
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
        Arc::new(move || {
            Box::pin(async move {
                let b: Arc<dyn ChatBackend> = Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn(text)]));
                Some((b, "local".to_string()))
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
        let r: FallbackResolver = Arc::new(|| Box::pin(async { None }));
        let b = FallbackBackend::new(Arc::new(Failing), r);
        assert!(collect(&b).await.is_err());
    }
}
