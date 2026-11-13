//! Benchmarks of the paths the app walks all day that need no model: the document search
//! (`kb_search`, keyword ranking over a synthetic corpus), the text chunker, and reading and
//! writing sessions and messages in the store (SQLite in WAL mode, a real file).
//!
//! Run with `cargo bench -p aiw-engine --bench hot_paths`; the CI only compiles it
//! (`cargo bench --no-run`). The numbers depend on the machine: see docs/benchmarks.md.
//! The corpus comes from a fixed-seed generator, so two runs measure the same data.

use aiw_core::api::{ChatMessage, KbAddBody, KbCollectionCreate, Session};
use aiw_engine::kb::chunk_text;
use aiw_engine::store::Store;
use aiw_engine::{Engine, EngineOptions};
use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

/// Small deterministic generator (xorshift64): the same corpus on every machine.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Pseudo-words: consonant-vowel syllables. `vocabulary` words, so rare terms exist.
fn vocabulary(rng: &mut Rng, n: usize) -> Vec<String> {
    const C: &[u8] = b"bcdfglmnprstvz";
    const V: &[u8] = b"aeiou";
    (0..n)
        .map(|_| {
            let syllables = 1 + rng.below(3);
            (0..syllables).flat_map(|_| [C[rng.below(C.len())] as char, V[rng.below(V.len())] as char]).collect()
        })
        .collect()
}

/// A document of roughly `bytes` bytes: headings and paragraphs of pseudo-words.
fn document(rng: &mut Rng, vocab: &[String], bytes: usize) -> String {
    let mut out = String::with_capacity(bytes + 200);
    let mut para = 0;
    while out.len() < bytes {
        if para % 5 == 0 {
            out.push_str("## ");
            for _ in 0..4 {
                out.push_str(&vocab[rng.below(vocab.len())]);
                out.push(' ');
            }
            out.push_str("\n\n");
        }
        let sentences = 2 + rng.below(4);
        for _ in 0..sentences {
            let words = 8 + rng.below(12);
            for w in 0..words {
                // Zipf-ish: low indexes are common words.
                let i = rng.below(vocab.len()).min(rng.below(vocab.len()));
                out.push_str(&vocab[i]);
                out.push(if w + 1 == words { '.' } else { ' ' });
            }
            out.push(' ');
        }
        out.push_str("\n\n");
        para += 1;
    }
    out
}

/// Writes `docs` Markdown files of about `bytes` bytes each into `dir`.
fn write_corpus(dir: &std::path::Path, rng: &mut Rng, vocab: &[String], docs: usize, bytes: usize) {
    for i in 0..docs {
        std::fs::write(dir.join(format!("doc-{i:05}.md")), document(rng, vocab, bytes)).unwrap();
    }
}

/// Indexes a folder the way the "add a folder" button does, and waits for the job to finish.
async fn add_folder(engine: &Engine, collection: &str, dir: &std::path::Path) {
    let r = engine.kb_add(collection, KbAddBody { files: None, path: Some(dir.to_string_lossy().into_owned()) }, true).await.unwrap();
    engine.kb_wait_idle().await;
    let job = engine.kb_job(&r.job.id).unwrap();
    assert_eq!(job.state, "done", "indexing the synthetic corpus failed: {job:?}");
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

// ---------------------------------------------------------------------------------------- kb

struct KbFixture {
    engine: Engine,
    _data: tempfile::TempDir,
    _ws: tempfile::TempDir,
    queries: Vec<String>,
}

/// An offline engine with one collection of `docs` documents of about `doc_bytes` bytes each.
fn kb_fixture(rt: &tokio::runtime::Runtime, docs: usize, doc_bytes: usize) -> KbFixture {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let vocab = vocabulary(&mut rng, 4000);
    let corpus = tempfile::tempdir().unwrap();
    write_corpus(corpus.path(), &mut rng, &vocab, docs, doc_bytes);
    // Queries: a common word, two words, a rare word and a phrase of three.
    let queries = vec![
        vocab[1].clone(),
        format!("{} {}", vocab[10], vocab[20]),
        vocab[3900].clone(),
        format!("{} {} {}", vocab[5], vocab[700], vocab[2500]),
    ];
    let engine = rt.block_on(async {
        let engine = Engine::new(EngineOptions {
            data_dir: Some(data.path().to_path_buf()),
            workspace_root: Some(ws.path().to_path_buf()),
            mode: "embedded".into(),
            offline: true,
            ..Default::default()
        })
        .await
        .unwrap();
        let col = engine.kb_create_collection(KbCollectionCreate { name: "bench".into(), description: None }).unwrap();
        add_folder(&engine, &col.id, corpus.path()).await;
        engine
    });
    KbFixture { engine, _data: data, _ws: ws, queries }
}

fn kb_search(c: &mut Criterion) {
    let rt = runtime();
    let mut g = c.benchmark_group("kb/search");
    g.sample_size(30);
    // (documents, bytes per document): about 0.4 MB and 4 MB of text.
    for (docs, bytes) in [(200usize, 2_000usize), (1_000, 4_000)] {
        let fx = kb_fixture(&rt, docs, bytes);
        for (n, q) in fx.queries.iter().enumerate() {
            g.bench_with_input(BenchmarkId::new(format!("{docs}_docs"), format!("query_{n}")), q, |b, q| {
                b.iter(|| rt.block_on(fx.engine.kb_search(black_box(q), None, 8)).unwrap())
            });
        }
    }
    g.finish();
}

fn kb_ingest(c: &mut Criterion) {
    let rt = runtime();
    let mut g = c.benchmark_group("kb/ingest");
    g.sample_size(10).measurement_time(Duration::from_secs(10));
    // 50 documents of 4 KB added to an empty collection: reading, chunking, hashing, FTS indexing.
    let corpus = tempfile::tempdir().unwrap();
    let mut rng = Rng(42);
    let vocab = vocabulary(&mut rng, 2000);
    write_corpus(corpus.path(), &mut rng, &vocab, 50, 4_000);
    g.bench_function("50_docs", |b| {
        b.iter_batched(
            || {
                let data = tempfile::tempdir().unwrap();
                let ws = tempfile::tempdir().unwrap();
                let (engine, id) = rt.block_on(async {
                    let e = Engine::new(EngineOptions {
                        data_dir: Some(data.path().to_path_buf()),
                        workspace_root: Some(ws.path().to_path_buf()),
                        mode: "embedded".into(),
                        offline: true,
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                    let col = e.kb_create_collection(KbCollectionCreate { name: "bench".into(), description: None }).unwrap();
                    (e, col.id)
                });
                (engine, id, data, ws)
            },
            |(engine, id, _data, _ws)| rt.block_on(add_folder(&engine, &id, corpus.path())),
            BatchSize::PerIteration,
        )
    });
    g.finish();
}

fn chunker(c: &mut Criterion) {
    let mut rng = Rng(7);
    let vocab = vocabulary(&mut rng, 3000);
    let mut g = c.benchmark_group("kb/chunk_text");
    for bytes in [10_000usize, 100_000, 1_000_000] {
        let text = document(&mut rng, &vocab, bytes);
        g.throughput(Throughput::Bytes(text.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(format!("{}_KB", bytes / 1000)), &text, |b, t| b.iter(|| chunk_text(black_box(t))));
    }
    g.finish();
}

// ------------------------------------------------------------------------------------- store

fn session(i: usize) -> Session {
    Session {
        id: format!("ses_{i:06}"),
        title: format!("Conversazione {i}"),
        agent_id: "operator".into(),
        created_at: "2026-01-01T10:00:00Z".into(),
        updated_at: "2026-01-01T10:00:00Z".into(),
        last_status: Some("succeeded".into()),
        worktree: None,
        channel: None,
    }
}

fn message(i: usize) -> ChatMessage {
    ChatMessage {
        id: format!("msg_{i:08}"),
        role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
        // About 600 bytes: a typical answer.
        content: format!("Messaggio {i}: ").repeat(40),
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        attachments: None,
        created_at: "2026-01-01T10:00:00Z".into(),
    }
}

fn store(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let st = Store::open(&dir.path().join("bench.db")).unwrap();
    let mut g = c.benchmark_group("store");

    // Write: a session row and its messages, the way a run saves them.
    let mut next = 0usize;
    g.bench_function("upsert_session", |b| {
        b.iter(|| {
            next += 1;
            st.upsert_session(&session(next)).unwrap()
        })
    });
    st.upsert_session(&session(0)).unwrap();
    let mut n = 0usize;
    g.bench_function("append_1_message", |b| {
        b.iter(|| {
            n += 1;
            st.append_messages("ses_000000", &[message(n)]).unwrap()
        })
    });
    for batch in [10usize, 100] {
        g.bench_function(format!("append_{batch}_messages"), |b| {
            b.iter(|| {
                let msgs: Vec<ChatMessage> = (0..batch)
                    .map(|_| {
                        n += 1;
                        message(n)
                    })
                    .collect();
                st.append_messages("ses_000000", &msgs).unwrap()
            })
        });
    }

    // Read: the list of sessions the sidebar shows, and a long conversation.
    for sessions in [100usize, 1_000] {
        let dir = tempfile::tempdir().unwrap();
        let st = Store::open(&dir.path().join("list.db")).unwrap();
        for i in 0..sessions {
            st.upsert_session(&session(i)).unwrap();
        }
        g.bench_with_input(BenchmarkId::new("list_sessions", sessions), &sessions, |b, _| b.iter(|| st.sessions().unwrap()));
    }
    for len in [100usize, 2_000] {
        let dir = tempfile::tempdir().unwrap();
        let st = Store::open(&dir.path().join("read.db")).unwrap();
        st.upsert_session(&session(1)).unwrap();
        let msgs: Vec<ChatMessage> = (0..len).map(message).collect();
        st.append_messages("ses_000001", &msgs).unwrap();
        g.bench_with_input(BenchmarkId::new("read_messages", len), &len, |b, _| b.iter(|| st.messages("ses_000001").unwrap()));
    }
    g.finish();
}

criterion_group!(benches, kb_search, kb_ingest, chunker, store);
criterion_main!(benches);
