//! Evaluation over HTTP: suites, runs in the background, reports, comparison.

use super::*;

const SUITE: &str =
    "id: api\nname: Suite API\ntasks:\n  - id: t\n    prompt: crea note.md\n    graders: [{ kind: file_exists, path: note.md }]\n";

#[tokio::test]
async fn suites_are_saved_from_json_or_text_and_validated() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let (st, list) = dispatch(app.clone(), "GET", "/api/evals", None).await;
    assert_eq!(st, 200);
    assert!(list.as_array().unwrap().iter().any(|s| s["id"] == "esempio"));

    let (st, saved) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": SUITE}))).await;
    assert_eq!(st, 200, "{saved}");
    assert_eq!(saved["tasks"][0]["id"], "t");
    // The JSON that came back is accepted as it is.
    let mut again = saved.clone();
    again["name"] = json!("Rinominata");
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(again)).await;
    assert_eq!((st, saved["name"].as_str()), (200, Some("Rinominata")));
    let (_, got) = dispatch(app.clone(), "GET", "/api/evals/api", None).await;
    assert_eq!(got["name"], "Rinominata");

    // Refused with the reason: wrong id, not a suite, bad grader, unknown agent.
    let (st, err) = dispatch(app.clone(), "PUT", "/api/evals/altro", Some(json!({"text": SUITE}))).await;
    assert_eq!(st, 400);
    assert!(err["error"].as_str().unwrap().contains("non corrisponde"), "{err}");
    let (st, _) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": "non è una suite"}))).await;
    assert_eq!(st, 400);
    let bad = SUITE.replace("file_exists, path: note.md", "judge, criteria: ' '");
    let (st, err) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": bad}))).await;
    assert_eq!(st, 400);
    assert!(err["error"].as_str().unwrap().contains("criteri"), "{err}");
    let (st, _) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": format!("{SUITE}agent: fantasma\n")}))).await;
    assert_eq!(st, 400);
    // A shell command cannot be written through the API.
    let cmd = SUITE.replace("{ kind: file_exists, path: note.md }", "{ kind: command, run: 'echo ciao' }");
    let (st, err) = dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": cmd}))).await;
    assert_eq!(st, 400);
    assert!(err["error"].as_str().unwrap().contains("command"), "{err}");

    let (st, _) = dispatch(app.clone(), "DELETE", "/api/evals/api", None).await;
    assert_eq!(st, 200);
    let (st, _) = dispatch(app.clone(), "GET", "/api/evals/api", None).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/evals/api", None).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn a_run_starts_in_the_background_and_its_report_is_polled_and_compared() {
    let (app, _e, _d, _w) = super::tests::app().await;
    dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": SUITE}))).await;

    let mut ids = vec![];
    for repeat in [1, 2] {
        let (st, started) = dispatch(app.clone(), "POST", "/api/evals/api/runs", Some(json!({"repeat": repeat}))).await;
        assert_eq!(st, 200, "{started}");
        assert_eq!(
            (started["status"].as_str(), started["suite_id"].as_str(), started["repeat"].as_u64()),
            (Some("running"), Some("api"), Some(repeat))
        );
        let id = started["id"].as_str().unwrap().to_string();
        let mut last = Value::Null;
        for _ in 0..300 {
            let (st, r) = dispatch(app.clone(), "GET", &format!("/api/eval-runs/{id}"), None).await;
            assert_eq!(st, 200);
            last = r;
            if last["status"] != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(last["status"], "completed", "{last}");
        assert_eq!(last["tasks"].as_array().unwrap().len() as u64, repeat);
        assert_eq!(last["summary"]["total"].as_u64(), Some(repeat));
        ids.push(id);
    }

    let (_, list) = dispatch(app.clone(), "GET", "/api/eval-runs", None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (st, c) = dispatch(app.clone(), "GET", &format!("/api/eval-runs/compare?a={}&b={}", ids[0], ids[1]), None).await;
    assert_eq!(st, 200, "{c}");
    assert_eq!((c["a"]["id"].as_str(), c["b"]["id"].as_str()), (Some(ids[0].as_str()), Some(ids[1].as_str())));
    assert_eq!(c["tasks"][0]["task_id"], "t");
    assert!(c["pass_rate_delta"].is_number());

    // Nothing is started when the request is wrong.
    for (path, body, want) in [
        ("/api/evals/api/runs", json!({"repeat": 0}), 400),
        ("/api/evals/api/runs", json!({"agent": "fantasma"}), 404),
        ("/api/evals/api/runs", json!({"profile": "fantasma"}), 404),
        ("/api/evals/nessuna/runs", json!({}), 404),
    ] {
        let (st, r) = dispatch(app.clone(), "POST", path, Some(body)).await;
        assert_eq!(st, want, "{path}: {r}");
    }
    let (_, list) = dispatch(app.clone(), "GET", "/api/eval-runs", None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    for path in ["/api/eval-runs/evr_none", "/api/eval-runs/compare?a=evr_none&b=evr_none"] {
        let (st, _) = dispatch(app.clone(), "GET", path, None).await;
        assert_eq!(st, 404, "{path}");
    }
    let (st, _) = dispatch(app.clone(), "GET", "/api/eval-runs/compare?a=solo", None).await;
    assert_eq!(st, 400);
}
