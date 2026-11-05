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
