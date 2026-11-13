//! Benchmarks of the server: how long a cold start takes (engine, then the HTTP listener) and the
//! latency of `GET /api/system`, the cheapest endpoint the UI calls on every start (there is no
//! dedicated health endpoint), over real TCP on 127.0.0.1, with and without connection reuse.
//!
//! No model is involved and the engine runs offline. Run with `cargo bench -p aiw-server --bench server`;
//! the CI only compiles it (`cargo bench --no-run`). The numbers depend on the machine: see docs/benchmarks.md.

use aiw_engine::{Engine, EngineOptions};
use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::time::Duration;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

async fn new_engine(data: &tempfile::TempDir, ws: &tempfile::TempDir) -> Engine {
    Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "daemon".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap()
}

/// Starts the real listener (`serve`, port chosen by the system) and returns its base URL and the task that runs it.
async fn start_listener(engine: &Engine) -> (String, tokio::task::JoinHandle<()>) {
    let e = engine.clone();
    let task = tokio::spawn(async move {
        let _ = aiw_server::serve(e, 0, false, None).await;
    });
    for _ in 0..5000 {
        if let Some(url) = engine.local_url() {
            return (url, task);
        }
        tokio::time::sleep(Duration::from_micros(200)).await;
    }
    panic!("the server did not start");
}

fn startup(c: &mut Criterion) {
    let rt = runtime();
    let mut g = c.benchmark_group("server/startup");
    g.sample_size(10).measurement_time(Duration::from_secs(15));
    // New data folder: the schema is created, the configuration and the registries are loaded, the hardware is probed.
    g.bench_function("engine_new_empty_data_dir", |b| {
        b.iter_batched(
            || (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()),
            |(data, ws)| {
                let e = rt.block_on(new_engine(&data, &ws));
                drop(e);
                (data, ws)
            },
            BatchSize::PerIteration,
        )
    });
    // The same folder opened again, as at every start after the first.
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    drop(rt.block_on(new_engine(&data, &ws)));
    g.bench_function("engine_new_existing_data_dir", |b| b.iter(|| drop(rt.block_on(new_engine(&data, &ws)))));
    // Everything between "the process starts" and "the first request is answered": the engine on an
    // existing data folder, the listener, and one `GET /api/system` on a new connection.
    let client = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    g.bench_function("cold_start_to_first_response", |b| {
        b.iter(|| {
            rt.block_on(async {
                let engine = new_engine(&data, &ws).await;
                let (base, task) = start_listener(&engine).await;
                let status = client.get(format!("{base}/api/system")).send().await.unwrap().status();
                assert!(status.is_success(), "GET /api/system answered {status}");
                task.abort();
                let _ = task.await;
            })
        })
    });
    // Router construction alone.
    let engine = rt.block_on(new_engine(&data, &ws));
    g.bench_function("router_build", |b| b.iter(|| aiw_server::router(engine.clone(), None)));
    g.finish();
}

fn latency(c: &mut Criterion) {
    let rt = runtime();
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let engine = rt.block_on(new_engine(&data, &ws));
    let (base, _server) = rt.block_on(start_listener(&engine));
    let url = format!("{base}/api/system");

    let reuse = reqwest::Client::new();
    let fresh = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    // One request first: it must answer 200 (a loopback caller needs no token), otherwise the numbers mean nothing.
    let status = rt.block_on(async { reuse.get(&url).send().await.unwrap().status() });
    assert!(status.is_success(), "GET /api/system answered {status}");

    let mut g = c.benchmark_group("server/get_api_system");
    g.sample_size(50);
    g.bench_function("keep_alive", |b| b.iter(|| rt.block_on(async { reuse.get(&url).send().await.unwrap().bytes().await.unwrap() })));
    g.bench_function("new_connection", |b| b.iter(|| rt.block_on(async { fresh.get(&url).send().await.unwrap().bytes().await.unwrap() })));
    g.finish();
}

criterion_group!(benches, startup, latency);
criterion_main!(benches);
