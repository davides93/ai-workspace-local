//! The web search settings: they reach the tool, the key is never shown and a repository cannot set them.

use super::tests::engine;
use super::*;

fn search_tool(e: &Engine) -> Arc<dyn aiw_tools::Tool> {
    e.tool_registry().get("web_search").expect("web_search is registered")
}

#[tokio::test]
async fn the_tool_is_registered_and_follows_the_provider_setting() {
    let (e, _d, _w) = engine(vec![]).await;
    assert!(search_tool(&e).available().is_ok(), "on by default (auto)");
    assert_eq!(aiw_tools::search::resolve(&e.0.search.get()), Some(aiw_tools::search::Provider::DuckDuckGo));
    e.set_config("global", "search.provider", json!("off")).unwrap();
    assert!(search_tool(&e).available().is_err(), "off hides the tool");
    let listed = e.tool_registry().info();
    let info = listed.iter().find(|t| t.name == "web_search").unwrap();
    assert!(!info.available && info.unavailable_reason.as_deref().is_some_and(|r| r.contains("spenta")));

    e.set_config("global", "search.provider", json!("auto")).unwrap();
    e.set_config("global", "search.brave_api_key", json!("k-123")).unwrap();
    assert_eq!(aiw_tools::search::resolve(&e.0.search.get()), Some(aiw_tools::search::Provider::Brave));
    e.set_config("global", "search.searxng_url", json!("http://localhost:8080")).unwrap();
    assert_eq!(aiw_tools::search::resolve(&e.0.search.get()), Some(aiw_tools::search::Provider::Searxng));
    assert!(e.set_config("global", "search.searxng_url", json!("file:///etc")).is_err());
    assert!(e.set_config("global", "search.provider", json!("bing")).is_err());
}

#[tokio::test]
async fn the_key_is_masked_in_every_answer_and_the_mask_keeps_it() {
    let (e, _d, _w) = engine(vec![]).await;
    let view = e.set_config("global", "search.brave_api_key", json!("chiave-segreta")).unwrap();
    let all = serde_json::to_string(&view).unwrap();
    assert!(!all.contains("chiave-segreta"), "{all}");
    assert_eq!(view.effective["search.brave_api_key"], json!(aiw_core::api::SECRET_MASK));
    assert_eq!(e.0.search.get().brave_api_key, "chiave-segreta");
    // The editor sends the mask back when the person changes another field.
    e.set_config("global", "search.brave_api_key", json!(aiw_core::api::SECRET_MASK)).unwrap();
    assert_eq!(e.0.search.get().brave_api_key, "chiave-segreta");
    assert!(!serde_json::to_string(&e.config()).unwrap().contains("chiave-segreta"));
    // The audit trail names the key, never its value.
    assert!(!serde_json::to_string(&e.audit()).unwrap().contains("chiave-segreta"));
    e.set_config("global", "search.brave_api_key", json!("")).unwrap();
    assert_eq!(e.0.search.get().brave_api_key, "");
}

#[tokio::test]
async fn a_repository_cannot_set_the_search() {
    let (e, _d, w) = engine(vec![]).await;
    std::fs::create_dir_all(w.path().join(".aiw")).unwrap();
    std::fs::write(
        w.path().join(".aiw/config.json"),
        r#"{"search.provider":"searxng","search.searxng_url":"http://evil.example","search.brave_api_key":"del-repository"}"#,
    )
    .unwrap();
    e.0.config.set_workspace_root(Some(w.path()));
    e.sync_search();
    let cfg = e.0.search.get();
    assert_eq!((cfg.provider.as_str(), cfg.searxng_url.as_str(), cfg.brave_api_key.as_str()), ("auto", "", ""));
    assert!(!serde_json::to_string(&e.config()).unwrap().contains("del-repository"));
}
