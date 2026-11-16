//! The credentials and the isolation of one agent over the real router (issue #71): who may reach the routes, that the value goes in
//! and never comes out, and that an editor that does not know the isolation cannot switch it off by saving.

use super::tests_security::{fixture, req, send, LAN};
use super::*;

const SECRET: &str = "TESTONLY-ROUTE-S3CR3T-CREDENTIAL-0001";

async fn call(app: &Router, method: &str, uri: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    let mut r = req(method, uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", auth.as_str())], body);
    r.extensions_mut().insert(security::TlsConn);
    let (st, _, v) = send(app, r).await;
    (st, v)
}

#[tokio::test]
async fn the_credential_routes_are_for_the_owner_the_value_goes_in_and_never_comes_out() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let owner = e.token().to_string();
    let device = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let gateway = e.create_api_token("App", "gateway", None, None, None).unwrap().token;
    let body = json!({"hosts": ["api.example.org"], "value": SECRET});
    // A paired phone, a third-party app and a stranger cannot read, create or remove a credential.
    for who in [&device, &gateway] {
        for (m, uri, b) in [
            ("GET", "/api/agents/planner/credentials", None),
            ("PUT", "/api/agents/planner/credentials/github", Some(body.clone())),
            ("DELETE", "/api/agents/planner/credentials/github", None),
        ] {
            let (st, v) = call(&app, m, uri, who, b).await;
            assert!(st == StatusCode::FORBIDDEN || st == StatusCode::UNAUTHORIZED, "{m} {uri} -> {st} {v}");
        }
    }
    let mut anonymous =
        req("PUT", "/api/agents/planner/credentials/github", Some(LAN), &[("host", "192.168.1.5:7777")], Some(body.clone()));
    anonymous.extensions_mut().insert(security::TlsConn);
    assert_eq!(send(&app, anonymous).await.0, StatusCode::UNAUTHORIZED);
    assert!(e.agent_credentials("planner").unwrap().is_empty(), "nothing was stored by the refused requests");
    // The owner creates, lists and removes. The answer is the metadata and the state: the value never comes back.
    let (st, created) = call(&app, "PUT", "/api/agents/planner/credentials/github", &owner, Some(body)).await;
    assert_eq!(st, StatusCode::OK, "{created}");
    assert_eq!(
        (created["name"].as_str(), created["set"].as_bool(), created["header"].as_str()),
        (Some("github"), Some(true), Some("Authorization"))
    );
    let (_, list) = call(&app, "GET", "/api/agents/planner/credentials", &owner, None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(!created.to_string().contains(SECRET) && !list.to_string().contains(SECRET));
    let (st, v) =
        call(&app, "PUT", "/api/agents/planner/credentials/cattiva", &owner, Some(json!({"hosts": ["com"], "value": SECRET}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(!v.to_string().contains(SECRET));
    let (st, _) =
        call(&app, "PUT", "/api/agents/nessuno/credentials/x", &owner, Some(json!({"hosts": ["a.example.org"], "value": SECRET}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The agent list a paired phone can read carries no credential and no value.
    let (_, agents) = call(&app, "GET", "/api/agents", &device, None).await;
    assert!(!agents.to_string().contains(SECRET) && !agents.to_string().contains("github"));
    let (st, _) = call(&app, "DELETE", "/api/agents/planner/credentials/github", &owner, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(e.agent_credentials("planner").unwrap().is_empty());
    let (st, _) = call(&app, "DELETE", "/api/agents/planner/credentials/github", &owner, None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // What the audit trail says about it carries no value.
    let audit = serde_json::to_string(&e.audit()).unwrap();
    assert!(audit.contains("agent.credential_set") && !audit.contains(SECRET));
}

#[tokio::test]
async fn an_editor_that_does_not_know_the_isolation_cannot_switch_it_off_by_saving() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let owner = e.token().to_string();
    let mut agent = serde_json::to_value(e.agent("planner").unwrap()).unwrap();
    agent["isolation"] = json!({"dedicated_workspace": true, "close_shell_network": false});
    let (st, saved) = call(&app, "PUT", "/api/agents/planner", &owner, Some(agent.clone())).await;
    assert_eq!(st, StatusCode::OK, "{saved}");
    assert_eq!(saved["isolation"]["dedicated_workspace"], true);
    // The old interface sends the profile without the field: the isolation stays.
    agent.as_object_mut().unwrap().remove("isolation");
    agent["description"] = json!("ritoccato");
    let (st, saved) = call(&app, "PUT", "/api/agents/planner", &owner, Some(agent.clone())).await;
    assert_eq!(st, StatusCode::OK, "{saved}");
    assert_eq!((saved["description"].as_str(), saved["isolation"]["dedicated_workspace"].as_bool()), (Some("ritoccato"), Some(true)));
    // The owner says it explicitly to switch it off.
    agent["isolation"] = json!({"dedicated_workspace": false, "close_shell_network": false});
    let (_, saved) = call(&app, "PUT", "/api/agents/planner", &owner, Some(agent)).await;
    assert!(saved.get("isolation").is_none_or(Value::is_null), "{saved}");
    // A paired phone cannot save an agent at all.
    let device = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let (st, _) = call(&app, "PUT", "/api/agents/planner", &device, Some(json!({}))).await;
    assert!(st == StatusCode::FORBIDDEN || st == StatusCode::UNAUTHORIZED);
}
