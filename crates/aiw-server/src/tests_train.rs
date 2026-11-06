//! Training over HTTP: trainers, doctor, datasets, jobs, adapters, rewards.

use super::*;

#[tokio::test]
async fn trainers_and_the_doctor_are_served_and_are_not_runtimes() {
    let (app, e, _d, _w) = super::tests::app().await;
    let (st, list) = dispatch(app.clone(), "GET", "/api/trainers", None).await;
    assert_eq!(st, 200, "{list}");
    let ids: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["mlx-lm-lora", "trl", "unsloth"]);
    assert!(list.as_array().unwrap().iter().all(|t| ["ready", "missing", "unsupported"].contains(&t["state"].as_str().unwrap())));
    // Not among the inference runtimes.
    let (_, rts) = dispatch(app.clone(), "GET", "/api/runtimes", None).await;
    assert!(rts.as_array().unwrap().iter().all(|r| !ids.contains(&r["id"].as_str().unwrap())));

    let (st, doc) = dispatch(app.clone(), "GET", "/api/train/doctor", None).await;
    assert_eq!(st, 200, "{doc}");
    assert_eq!(doc["device"]["tier"], json!(e.device().tier));
    assert_eq!(doc["trainers"].as_array().unwrap().len(), 3);
    assert!(doc["can_train"].is_array() && doc["advice"].is_array());

    // The plan of a trainer is served; an unknown one is a 404.
    let (st, plan) = dispatch(app.clone(), "GET", "/api/trainers/trl/install-plan", None).await;
    assert_eq!(st, 200, "{plan}");
    assert_eq!(plan["runtime_id"], "trl");
    let (st, _) = dispatch(app.clone(), "GET", "/api/trainers/nope/install-plan", None).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app, "GET", "/api/trainers/trl/install", None).await;
    assert_eq!(st, 200);
}

#[tokio::test]
async fn datasets_jobs_and_adapters_are_served_and_their_errors_are_http_errors() {
    let (app, _e, _d, _w) = super::tests::app().await;
    for path in ["/api/datasets", "/api/train/jobs", "/api/adapters"] {
        let (st, list) = dispatch(app.clone(), "GET", path, None).await;
        assert_eq!((st, list), (200, json!([])), "{path}");
    }
    // Too few sessions: the request is wrong, not the server.
    let (st, err) = dispatch(app.clone(), "POST", "/api/datasets", Some(json!({"method": "sft"}))).await;
    assert_eq!(st, 400, "{err}");
    assert!(err.to_string().contains("almeno 10"), "{err}");
    let (st, _) = dispatch(app.clone(), "POST", "/api/datasets", Some(json!({"method": "boh"}))).await;
    assert_eq!(st, 400);
    for path in ["/api/datasets/ds_none", "/api/train/jobs/trn_none", "/api/train/jobs/trn_none/log", "/api/adapters/ad_none"] {
        let (st, _) = dispatch(app.clone(), "GET", path, None).await;
        assert_eq!(st, 404, "{path}");
    }
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/datasets/ds_none", None).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/adapters/ad_none", None).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app.clone(), "POST", "/api/train/jobs/trn_none/cancel", None).await;
    assert_eq!(st, 404);
    let body = json!({"method": "sft", "base_model": "m", "dataset_id": "ds_none"});
    let (st, _) = dispatch(app, "POST", "/api/train/jobs", Some(body)).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn the_reward_endpoint_scores_answers_and_refuses_graders_that_need_files() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let suite = "id: somme\nname: Somme\ntasks:\n  - id: due\n    prompt: quanto fa 1+1?\n    graders: [{ kind: output_matches, pattern: '^2$' }]\n  - id: nota\n    prompt: crea nota.md\n    graders: [{ kind: file_exists, path: nota.md }]\n";
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/evals/somme", Some(json!({"text": suite}))).await;
    assert_eq!(st, 200, "{saved}");

    let (st, good) = dispatch(app.clone(), "POST", "/api/evals/somme/reward", Some(json!({"task_id": "due", "output": "2"}))).await;
    assert_eq!(st, 200, "{good}");
    assert_eq!((good["score"].as_f64(), good["passed"].as_bool()), (Some(1.0), Some(true)));
    assert_eq!(good["graders"][0]["kind"], "output_matches");
    let (_, bad) = dispatch(app.clone(), "POST", "/api/evals/somme/reward", Some(json!({"task_id": "due", "output": "5"}))).await;
    assert_eq!((bad["score"].as_f64(), bad["passed"].as_bool()), (Some(0.0), Some(false)));

    let (st, err) = dispatch(app.clone(), "POST", "/api/evals/somme/reward", Some(json!({"task_id": "nota", "output": "fatto"}))).await;
    assert_eq!(st, 400);
    assert!(err["error"].as_str().unwrap().contains("file_exists"), "{err}");
    let (st, _) = dispatch(app.clone(), "POST", "/api/evals/somme/reward", Some(json!({"task_id": "boh", "output": "x"}))).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app.clone(), "POST", "/api/evals/nessuna/reward", Some(json!({"task_id": "due", "output": "x"}))).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app, "POST", "/api/evals/somme/reward", Some(json!({"task_id": "due"}))).await;
    assert_eq!(st, 400);
}
