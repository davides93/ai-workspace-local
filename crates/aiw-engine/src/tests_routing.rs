//! The bandit router: candidates, the choice of a profile for a run, rewards from thumbs and evaluations.

use super::tests::{engine, wait_finished};
use super::*;
use aiw_runtimes::ScriptedBackend;

/// An agent that may use the profiles `default` and `alt`, with the router on.
async fn routed(turns: usize) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let script = (0..turns).map(|i| ScriptedBackend::text_turn(&format!("risposta {i}"))).collect();
    let (e, d, w) = engine(script).await;
    e.save_profile(InferenceProfile {
        id: "alt".into(),
        name: "Alt".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted-alt".into(),
        params: Default::default(),
    })
    .unwrap();
    let base = e.agent("operator").unwrap();
    e.save_agent(AgentProfile { id: "mio".into(), name: "Mio".into(), candidate_profiles: vec!["default".into(), "alt".into()], ..base })
        .unwrap();
    e.set_config("global", "routing.bandit", json!(true)).unwrap();
    (e, d, w)
}

/// One exchange in `session`; returns the run and the id of the answer.
async fn ask(e: &Engine, session: &str) -> (String, String) {
    let run = e.start_run(session, "ciao".into()).await.unwrap();
    wait_finished(e, &run).await;
    let answer = e.messages(session).unwrap().into_iter().rev().find(|m| m.role == "assistant").unwrap().id;
    (run, answer)
}

/// What the router said about a run: the profile of the `aiw.route` event, its reason, and the model of the snapshot.
fn routing_of(e: &Engine, run: &str) -> (Option<String>, Option<String>, Option<String>) {
    let events = e.run_events(run);
    let route = events.iter().find_map(|ev| match &ev.event.event {
        AgEvent::Custom { name, value } if name == "aiw.route" => {
            Some((value["profile"].as_str().map(String::from), value["reason"].as_str().map(String::from)))
        }
        _ => None,
    });
    let model = events.iter().find_map(|ev| match &ev.event.event {
        AgEvent::StateSnapshot { snapshot } => snapshot.model.clone(),
        _ => None,
    });
    let (profile, reason) = route.unzip();
    (profile.flatten(), reason.flatten(), model)
}

fn stat(e: &Engine, profile: &str) -> (u64, f64) {
    let s = e.routing_stats("mio").unwrap();
    let c = s.candidates.iter().find(|c| c.profile == profile).unwrap();
    (c.n, c.reward_sum)
}

#[tokio::test]
async fn candidates_must_exist_and_be_listed_once() {
    let (e, _d, _w) = engine(vec![]).await;
    let base = e.agent("operator").unwrap();
    let save = |c: Vec<&str>| {
        e.save_agent(AgentProfile { id: "x".into(), candidate_profiles: c.into_iter().map(String::from).collect(), ..base.clone() })
    };
    assert!(save(vec!["default"]).is_ok());
    assert!(save(vec!["default", "default"]).unwrap_err().to_string().contains("due volte"));
    assert!(matches!(save(vec!["fantasma"]), Err(Error::Invalid(_))));
    // Without candidates the stats are empty and the router is off by default.
    let s = e.routing_stats("operator").unwrap();
    assert!(!s.enabled && s.candidates.is_empty());
    assert!(matches!(e.routing_stats("nessuno"), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn the_router_tries_every_profile_and_then_follows_the_thumbs() {
    let (e, _d, _w) = routed(8).await;
    let s = e.create_session("mio", None).unwrap();

    // The first run takes the first candidate that was never tried, and says so.
    let (run1, answer1) = ask(&e, &s.id).await;
    assert_eq!(routing_of(&e, &run1), (Some("default".into()), Some("mai provato".into()), Some("scripted".into())));
    // A thumb up is a reward of 1 for the profile that wrote the answer.
    e.set_feedback(&s.id, &answer1, 1, None).unwrap();
    assert_eq!(stat(&e, "default"), (1, 1.0));

    let (run2, answer2) = ask(&e, &s.id).await;
    assert_eq!(routing_of(&e, &run2).0.as_deref(), Some("alt"), "the other one was never tried");
    assert_eq!(routing_of(&e, &run2).2.as_deref(), Some("scripted-alt"), "the run really used it");
    e.set_feedback(&s.id, &answer2, -1, None).unwrap();
    assert_eq!(stat(&e, "alt"), (1, 0.0));

    // Both tried once: the one with the thumb up wins.
    let (run3, _) = ask(&e, &s.id).await;
    let (profile, reason, _) = routing_of(&e, &run3);
    assert_eq!(profile.as_deref(), Some("default"));
    assert!(reason.unwrap().starts_with("UCB1: media 1.00"));

    // Changing a thumb replaces its reward instead of adding one.
    e.set_feedback(&s.id, &answer2, 1, None).unwrap();
    assert_eq!(stat(&e, "alt"), (1, 1.0));
    // Taking it back removes it.
    e.delete_feedback(&s.id, &answer2).unwrap();
    assert_eq!(stat(&e, "alt"), (0, 0.0));
    let (run4, _) = ask(&e, &s.id).await;
    assert_eq!(routing_of(&e, &run4).0.as_deref(), Some("alt"), "untried again");

    // Forgetting starts over.
    e.reset_routing("mio").unwrap();
    assert_eq!((stat(&e, "default"), stat(&e, "alt")), ((0, 0.0), (0, 0.0)));
}

#[tokio::test]
async fn without_the_setting_or_candidates_the_agent_keeps_its_own_profile() {
    let (e, _d, _w) = routed(4).await;
    e.set_config("global", "routing.bandit", json!(false)).unwrap();
    let s = e.create_session("mio", None).unwrap();
    let (run, answer) = ask(&e, &s.id).await;
    assert_eq!(routing_of(&e, &run), (None, None, Some("scripted".into())));
    // A thumb on a run that was not routed rewards nobody.
    e.set_feedback(&s.id, &answer, 1, None).unwrap();
    assert_eq!(stat(&e, "default"), (0, 0.0));

    e.set_config("global", "routing.bandit", json!(true)).unwrap();
    let op = e.create_session("operator", None).unwrap();
    let (run, _) = ask(&e, &op.id).await;
    assert_eq!(routing_of(&e, &run).0, None, "an agent without candidates is never routed");
}

#[tokio::test]
async fn passed_and_failed_evaluation_tasks_are_rewards_for_the_profile_they_ran_with() {
    let (e, _d, _w) = routed(0).await;
    let suite = aiw_eval::Suite::parse(
        "id: s\nname: S\nagent: mio\ntasks:\n  - id: a\n    prompt: x\n    graders: [{ kind: output_matches, pattern: x }]\n  - id: b\n    prompt: y\n    graders: [{ kind: output_matches, pattern: y }]\n",
    )
    .unwrap();
    let task = |id: &str, passed: bool, run: Option<&str>| aiw_eval::TaskResult {
        task_id: id.into(),
        attempt: 1,
        passed,
        score: if passed { 1.0 } else { 0.0 },
        graders: vec![],
        duration_ms: 1,
        tokens: 1,
        turns: 1,
        run_id: run.map(String::from),
        error: None,
        workdir: None,
    };
    let report = aiw_eval::EvalReport {
        id: "evr_1".into(),
        suite_id: "s".into(),
        suite_name: "S".into(),
        status: "completed".into(),
        started_at: now_rfc3339(),
        finished_at: None,
        agent: None,
        profile: Some("alt".into()),
        repeat: 1,
        tasks: vec![task("a", true, Some("run_a")), task("b", false, Some("run_b"))],
        summary: Default::default(),
        error: None,
    };
    e.rewards_from_report(&suite, None, Some("alt"), &report);
    assert_eq!((stat(&e, "alt"), stat(&e, "default")), ((2, 1.0), (0, 0.0)));
    // The same report again does not count twice, and a profile that is not a candidate earns nothing.
    e.rewards_from_report(&suite, None, Some("alt"), &report);
    assert_eq!(stat(&e, "alt"), (2, 1.0));
    e.rewards_from_report(&suite, None, Some("train-xyz"), &report);
    assert_eq!(stat(&e, "alt"), (2, 1.0));
}
