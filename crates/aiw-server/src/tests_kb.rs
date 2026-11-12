//! The knowledge base over HTTP (collections, documents, search, limits, 404s, the path rule for
//! remote callers) and `POST /v1/embeddings` with a fake embedding backend and the 503 path.

use super::*;
use aiw_runtimes::ChatBackend;
use async_trait::async_trait;
use axum::http::HeaderMap;
use base64::Engine as _;
use std::sync::Arc;

struct Themes;

#[async_trait]
impl ChatBackend for Themes {
    async fn list_models(&self) -> aiw_core::Result<Vec<String>> {
        Ok(vec![])
    }
    async fn chat(
        &self,
        _: aiw_runtimes::ChatRequest,
        _: tokio_util::sync::CancellationToken,
    ) -> aiw_core::Result<aiw_runtimes::DeltaStream> {
        Err(Error::Unsupported("solo embedding".into()))
    }
    async fn embed(&self, _model: &str, texts: &[String]) -> aiw_core::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| vec![t.chars().count() as f32, if t.contains("gatto") { 1.0 } else { 0.0 }, 0.5]).collect())
    }
}

fn file(name: &str, bytes: &[u8]) -> Value {
    json!({"name": name, "mime": "", "data": base64::engine::general_purpose::STANDARD.encode(bytes)})
}

async fn idle(e: &Engine) {
    e.kb_wait_idle().await;
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    peer: Option<[u8; 4]>,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut b = axum::http::Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            b = b.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut r = b.body(body).unwrap();
    if let Some(ip) = peer {
        r.extensions_mut().insert(ConnectInfo(SocketAddr::from((ip, 40000))));
    }
    let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn collections_documents_and_search_work_end_to_end() {
    let (app, e, _d, _w) = super::tests::app().await;
    let (st, list) = dispatch(app.clone(), "GET", "/api/kb/collections", None).await;
    assert_eq!((st, list.as_array().map(|l| l.len())), (200, Some(0)));

    let (st, c) =
        dispatch(app.clone(), "POST", "/api/kb/collections", Some(json!({"name": "Contratti", "description": "Affitto e utenze"}))).await;
    assert_eq!(st, 200, "{c}");
    let id = c["id"].as_str().unwrap().to_string();
    assert_eq!((c["name"].as_str(), c["documents"].as_u64()), (Some("Contratti"), Some(0)));
    // Errors: duplicate name 409, empty name 400, unknown id 404.
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/collections", Some(json!({"name": "contratti"}))).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/collections", Some(json!({"name": " "}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/collections", Some(json!({"nome": "x"}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "PATCH", "/api/kb/collections/kbk_none", Some(json!({"name": "x"}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/kb/collections/kbk_none", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "GET", "/api/kb/collections/kbk_none/documents", None).await.0, 404);
    assert_eq!(
        dispatch(app.clone(), "POST", "/api/kb/collections/kbk_none/documents", Some(json!({"files": [file("a.txt", b"x")]}))).await.0,
        404
    );
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/collections/kbk_none/reindex", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/kb/documents/kbd_none", None).await.0, 404);

    // Rename.
    let (st, p) = dispatch(app.clone(), "PATCH", &format!("/api/kb/collections/{id}"), Some(json!({"name": "Casa"}))).await;
    assert_eq!((st, p["name"].as_str(), p["description"].as_str()), (200, Some("Casa"), Some("Affitto e utenze")));

    // Add files: the answer carries the job, the documents appear when it is done.
    let body = json!({"files": [file("contratto.txt", b"Il canone mensile e di 500 euro."), file("note.md", b"# Note\n\nLa caparra e di mille euro."), file("foto.png", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0])]});
    let (st, added) = dispatch(app.clone(), "POST", &format!("/api/kb/collections/{id}/documents"), Some(body)).await;
    assert_eq!(st, 200, "{added}");
    assert_eq!(added["job"]["total"], 2);
    assert_eq!(added["job"]["skipped"][0]["name"], "foto.png");
    idle(&e).await;
    let (_, docs) = dispatch(app.clone(), "GET", &format!("/api/kb/collections/{id}/documents"), None).await;
    let docs = docs.as_array().unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.iter().all(|d| d["status"] == "ready" && d["source"] == "caricato" && d["chunks"].as_u64().unwrap() >= 1));
    let (_, status) = dispatch(app.clone(), "GET", "/api/kb/status", None).await;
    assert_eq!(status["jobs"][0]["state"], "done");
    assert!(status.get("embedding_model").is_none(), "no embedding model on this engine");
    let (_, cols) = dispatch(app.clone(), "GET", "/api/kb/collections", None).await;
    assert_eq!((cols[0]["documents"].as_u64(), cols[0]["chunks"].as_u64()), (Some(2), Some(2)));

    // Same content again: nothing added.
    let (_, again) = dispatch(
        app.clone(),
        "POST",
        &format!("/api/kb/collections/{id}/documents"),
        Some(json!({"files": [file("copia.txt", b"Il canone mensile e di 500 euro.")]})),
    )
    .await;
    idle(&e).await;
    assert_eq!(e.kb_job(again["job"]["id"].as_str().unwrap()).unwrap().unchanged, 1);

    // Search.
    let (st, r) = dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "canone mensile"}))).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["semantic"], false);
    assert_eq!(
        (r["hits"][0]["document_name"].as_str(), r["hits"][0]["part"].as_u64(), r["hits"][0]["collection_name"].as_str()),
        (Some("contratto.txt"), Some(1), Some("Casa"))
    );
    assert!(r["hits"][0]["score"].as_f64().unwrap() > 0.0 && r["hits"][0]["text"].as_str().unwrap().contains("500 euro"));
    let (_, one) =
        dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "euro", "collection": "casa", "max_results": 1}))).await;
    assert_eq!(one["hits"].as_array().unwrap().len(), 1);
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "  "}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "x", "collection": "nessuna"}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({}))).await.0, 400);

    // Reindex answers with a job; cancelling an unknown one is a 404.
    let (st, re) = dispatch(app.clone(), "POST", &format!("/api/kb/collections/{id}/reindex"), None).await;
    assert_eq!((st, re["job"]["total"].as_u64()), (200, Some(0)), "uploaded files have nothing to read again");
    assert_eq!(dispatch(app.clone(), "POST", "/api/kb/jobs/kbj_none/cancel", None).await.0, 404);

    // Delete a document, then the collection: the passages are gone from search.
    let doc_id = docs.iter().find(|d| d["name"] == "contratto.txt").unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/kb/documents/{doc_id}"), None).await.0, 200);
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/kb/documents/{doc_id}"), None).await.0, 404);
    let (_, none) = dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "canone"}))).await;
    assert!(none["hits"].as_array().unwrap().is_empty());
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/kb/collections/{id}"), None).await.0, 200);
    let (_, none) = dispatch(app.clone(), "POST", "/api/kb/search", Some(json!({"query": "caparra"}))).await;
    assert!(none["hits"].as_array().unwrap().is_empty());
    assert_eq!(e.kb_collections().unwrap().len(), 0);
}

#[tokio::test]
async fn limits_are_those_of_chat_attachments_and_errors_are_in_italian() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let (_, c) = dispatch(app.clone(), "POST", "/api/kb/collections", Some(json!({"name": "Limiti"}))).await;
    let uri = format!("/api/kb/collections/{}/documents", c["id"].as_str().unwrap());
    let many: Vec<Value> = (0..9).map(|i| file(&format!("{i}.txt"), b"ciao")).collect();
    let (st, e1) = dispatch(app.clone(), "POST", &uri, Some(json!({"files": many}))).await;
    assert_eq!(st, 400);
    assert!(e1["error"].as_str().unwrap().contains("al massimo 8 file per volta"), "{e1}");
    let big = vec![b'a'; 10 * 1024 * 1024 + 1];
    let (st, e2) = dispatch(app.clone(), "POST", &uri, Some(json!({"files": [file("grande.txt", &big)]}))).await;
    assert_eq!(st, 400);
    assert!(e2["error"].as_str().unwrap().contains("troppo grande"), "{e2}");
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(json!({}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(json!({"files": [file("a.txt", b"x")], "path": "/tmp"}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(json!({"path": std::env::temp_dir().join("aiw-non-esiste-proprio").to_string_lossy()}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(json!({"path": "relativo"}))).await.0, 400);
    // The body limit of this route is the one of the chat attachments: a request that carries
    // well over 25 MB of files is refused with 413 before it is read.
    let huge = "A".repeat(40 << 20);
    let (st, _) = dispatch(app.clone(), "POST", &uri, Some(json!({"files": [{"name": "a.txt", "data": huge}]}))).await;
    assert_eq!(st, 413);
}

#[tokio::test]
async fn a_path_is_read_only_for_this_device_and_a_remote_one_can_still_send_files() {
    let (app, e, _d, _w) = super::tests::app().await;
    let c = e.kb_create_collection(KbCollectionCreate { name: "Locale".into(), description: None }).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# A\n\nTesto sul gatto.").unwrap();
    let uri = format!("/api/kb/collections/{}/documents", c.id);
    let path_body = json!({"path": dir.path().to_string_lossy()});
    let token = e.token().to_string();
    // A paired device on the LAN (valid token): files yes, a path on this computer no.
    let lan = Some([192, 168, 1, 50]);
    let (st, _, v) = send(&app, "POST", &uri, lan, Some(&token), Some(path_body.clone())).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("solo da questo dispositivo"));
    assert!(e.kb_documents(&c.id).unwrap().is_empty());
    let (st, _, v) = send(&app, "POST", &uri, lan, Some(&token), Some(json!({"files": [file("remoto.txt", b"testo")]}))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    idle(&e).await;
    // Without the token a remote caller gets nowhere.
    let (st, _, _) = send(&app, "POST", &uri, lan, None, Some(path_body.clone())).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // This device (loopback, and the in-process shell) may.
    let (st, _, v) = send(&app, "POST", &uri, Some([127, 0, 0, 1]), None, Some(path_body.clone())).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    idle(&e).await;
    let (st, _, _) = send(&app, "POST", &uri, None, None, Some(path_body)).await;
    assert_eq!(st, StatusCode::OK);
    idle(&e).await;
    let names: Vec<String> = e.kb_documents(&c.id).unwrap().into_iter().map(|d| d.name).collect();
    assert_eq!(names, ["remoto.txt", "a.md"], "the second path add found the same file: nothing new");
}

#[tokio::test]
async fn the_embeddings_endpoint_answers_in_the_openai_shape_with_the_memory_embedding_model() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.use_embedder(Some(Arc::new(Themes)), "fake-embed");
    let (st, v) =
        dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"model": "text-embedding-3-small", "input": "il mio gatto"}))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["object"].as_str(), v["model"].as_str()), (Some("list"), Some("text-embedding-3-small")));
    assert_eq!(v["data"][0]["object"], "embedding");
    assert_eq!(v["data"][0]["index"], 0);
    assert_eq!(v["data"][0]["embedding"], json!([12.0, 1.0, 0.5]));
    assert_eq!(v["usage"], json!({"prompt_tokens": 3, "total_tokens": 3}));

    // A list, no model named (the answer names the one that was used).
    let (st, v) = dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"input": ["uno", "il gatto dorme"]}))).await;
    assert_eq!(st, 200);
    assert_eq!(v["model"], "fake-embed");
    assert_eq!(v["data"].as_array().unwrap().len(), 2);
    assert_eq!((v["data"][1]["index"].as_u64(), v["data"][1]["embedding"][1].as_f64()), (Some(1), Some(1.0)));

    // base64: little-endian f32, what the OpenAI Python client asks for.
    let (_, v) = dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"input": "ab", "encoding_format": "base64"}))).await;
    let bytes = base64::engine::general_purpose::STANDARD.decode(v["data"][0]["embedding"].as_str().unwrap()).unwrap();
    let floats: Vec<f32> = bytes.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    assert_eq!(floats, [2.0, 0.0, 0.5]);

    // Bad requests are 400 in Italian.
    for bad in [
        json!({"input": ""}),
        json!({"input": []}),
        json!({"input": [1, 2, 3]}),
        json!({"input": [[1, 2]]}),
        json!({"input": 5}),
        json!({"input": "x", "encoding_format": "hex"}),
        json!({"model": "m"}),
    ] {
        let (st, v) = dispatch(app.clone(), "POST", "/v1/embeddings", Some(bad.clone())).await;
        assert_eq!(st, 400, "{bad} → {v}");
        assert!(v["error"].is_string());
    }
    let many: Vec<String> = (0..257).map(|i| format!("t{i}")).collect();
    assert_eq!(dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"input": many}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/v1/embeddings", None).await.0, 405);
}

#[tokio::test]
async fn without_an_embedding_model_the_embeddings_endpoint_is_a_503_with_an_explanation() {
    let (app, e, _d, _w) = super::tests::app().await;
    assert!(e.embedding_model_name().is_none());
    let (st, v) = dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"input": "ciao"}))).await;
    assert_eq!(st, 503, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Nessun modello di embedding è pronto"), "{v}");
    // Naming a profile that cannot embed does not make one appear.
    let (st, _) = dispatch(app.clone(), "POST", "/v1/embeddings", Some(json!({"model": "default", "input": "ciao"}))).await;
    assert_eq!(st, 503);
}

#[tokio::test]
async fn the_embeddings_endpoint_needs_the_same_token_as_the_chat_completions() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.use_embedder(Some(Arc::new(Themes)), "fake-embed");
    let body = json!({"input": "ciao"});
    let lan = Some([192, 168, 1, 50]);
    let chat = json!({"model": "default", "messages": [{"role": "user", "content": "ciao"}]});
    let (chat_st, _, _) = send(&app, "POST", "/v1/chat/completions", lan, None, Some(chat)).await;
    let (st, _, _) = send(&app, "POST", "/v1/embeddings", lan, None, Some(body.clone())).await;
    assert_eq!((chat_st, st), (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED));
    let (st, _, v) = send(&app, "POST", "/v1/embeddings", lan, Some(e.token()), Some(body.clone())).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _, _) = send(&app, "POST", "/v1/embeddings", Some([127, 0, 0, 1]), None, Some(body)).await;
    assert_eq!(st, StatusCode::OK, "loopback needs no token, like the chat");
}
