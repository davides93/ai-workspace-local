//! `GET /api/recommend` (the model adviser) and `auto:*` agent profiles over HTTP.

use super::tests::{app, app_on, reqwest_like_get};
use super::*;
use aiw_core::api::{Accelerator, CpuInfo};

fn device(ram_gb: u64, vram_gb: Option<u64>) -> DeviceProfile {
    let mut accelerators =
        vec![Accelerator { kind: "cpu".into(), name: "cpu".into(), vendor: None, memory_mb: None, driver: None, details: None }];
    if let Some(v) = vram_gb {
        accelerators.push(Accelerator {
            kind: "cuda".into(),
            name: "GPU".into(),
            vendor: None,
            memory_mb: Some(v * 1024),
            driver: None,
            details: None,
        });
    }
    DeviceProfile {
        id: "d".into(),
        name: "d".into(),
        os: "linux".into(),
        os_version: String::new(),
        arch: "x86_64".into(),
        form_factor: "desktop".into(),
        cpu: CpuInfo { brand: "cpu".into(), cores: 8, threads: 16, features: vec![] },
        memory_mb: ram_gb * 1024,
        available_memory_mb: ram_gb * 1024,
        accelerators,
        tier: String::new(),
    }
}

#[tokio::test]
async fn recommend_returns_one_entry_per_task_with_a_download_that_fits() {
    let (app, _e, _d, _w) = app_on(Some(device(16, None))).await;
    let (st, v) = dispatch(app, "GET", "/api/recommend", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["tier"], "m");
    for task in ["chat", "agent", "reasoning", "vision", "embeddings"] {
        assert!(v[task].is_object(), "{task}: {v}");
        assert!(v[task]["installed"].is_null(), "nothing is installed in a test: {task}");
    }
    let dl = &v["agent"]["download"];
    assert_eq!(dl["catalog_id"], "qwen3-8b");
    assert!(["great", "ok"].contains(&dl["fit"].as_str().unwrap()));
    assert!(dl["reason"].as_str().unwrap().contains("strumenti"));
    assert!(dl["context"].as_u64().unwrap() >= 2048);
    assert_eq!(v["reasoning"]["download"]["thinking"], "deep");
    // The device class changes the answer.
    let (app, _e, _d, _w) = app_on(Some(device(8, None))).await;
    let (_, small) = dispatch(app, "GET", "/api/recommend", None).await;
    assert_eq!(small["tier"], "s");
    assert_eq!(small["agent"]["download"]["catalog_id"], "llama-3.2-3b");
}

#[tokio::test]
async fn recommend_follows_the_same_auth_as_the_other_routes() {
    let (app, e, _d, _w) = app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let lan_app = app.layer(axum::middleware::from_fn(|mut req: Request, next: Next| async move {
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 50], 5555))));
        next.run(req).await
    }));
    tokio::spawn(async move { axum::serve(listener, lan_app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    let url = format!("http://{addr}/api/recommend");
    assert_eq!(reqwest_like_get(&url, None).await.0, 401);
    let (st, v) = reqwest_like_get(&url, Some(e.token())).await;
    assert_eq!(st, 200);
    assert!(v["agent"].is_object());
}

#[tokio::test]
async fn an_auto_agent_without_any_model_answers_409_in_italian() {
    let (app, _e, _d, _w) = app().await;
    // The seeded agents follow the adviser; the hand-made `default` profile is their fallback.
    let (_, agents) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    let planner = agents.as_array().unwrap().iter().find(|a| a["id"] == "planner").unwrap();
    assert_eq!(planner["inference_profile"], "auto:reasoning");
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/profiles/default", None).await;
    assert_eq!(st, 200);
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "planner"}))).await;
    let sid = ses["id"].as_str().unwrap();
    let (st, err) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "ciao"}))).await;
    assert_eq!(st, 409, "{err}");
    assert!(err["error"].as_str().unwrap().contains("scaricane uno da Modelli"), "{err}");
    // An agent may be saved with an automatic profile even now.
    let (_, ops) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    let mut op = ops.as_array().unwrap().iter().find(|a| a["id"] == "operator").unwrap().clone();
    op["inference_profile"] = json!("auto:chat");
    let (st, saved) = dispatch(app, "PUT", "/api/agents/operator", Some(op)).await;
    assert_eq!(st, 200, "{saved}");
}
