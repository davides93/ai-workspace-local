//! The knowledge base: chunking, ranking, ingestion (text, PDF, folders, dedup, replace), search
//! with and without embeddings, the agent tool and its scoping, deletion.

use super::kb::*;
use super::memory::{semantic_rank, Embedder};
use super::tests::{engine, wait_finished};
use super::tests_attachments::tiny_pdf;
use super::*;
use aiw_core::api::{AgentProfile, AttachmentUpload, KbAddBody, KbCollectionCreate, KbHit};
use aiw_runtimes::{ChatBackend, ScriptedBackend};
use async_trait::async_trait;
use base64::Engine as _;

fn up(name: &str, bytes: &[u8]) -> AttachmentUpload {
    AttachmentUpload { name: name.into(), mime: String::new(), data: base64::engine::general_purpose::STANDARD.encode(bytes) }
}

/// Embeds by topic: one axis per theme, so similarity is predictable.
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
        let axes: [&[&str]; 3] =
            [&["gatto", "gatti", "micio", "felino"], &["caffe", "caffè", "espresso", "bevanda"], &["canone", "affitto", "contratto"]];
        Ok(texts
            .iter()
            .map(|t| {
                let t = t.to_lowercase();
                axes.iter().map(|ws| if ws.iter().any(|w| t.contains(w)) { 1.0 } else { 0.0 }).chain([0.05]).collect()
            })
            .collect())
    }
}

fn themes() -> Embedder {
    Embedder::Fixed(Arc::new(Themes), "temi".into())
}

fn col(e: &Engine, name: &str) -> aiw_core::api::KbCollection {
    e.kb_create_collection(KbCollectionCreate { name: name.into(), description: None }).unwrap()
}

async fn add_files(e: &Engine, c: &str, files: Vec<AttachmentUpload>) -> aiw_core::api::KbJob {
    let r = e.kb_add(c, KbAddBody { files: Some(files), path: None }, true).await.unwrap();
    e.kb_wait_idle().await;
    e.kb_job(&r.job.id).unwrap()
}

async fn add_path(e: &Engine, c: &str, path: &std::path::Path) -> aiw_core::api::KbJob {
    let r = e.kb_add(c, KbAddBody { files: None, path: Some(path.to_string_lossy().into_owned()) }, true).await.unwrap();
    e.kb_wait_idle().await;
    e.kb_job(&r.job.id).unwrap()
}

fn hit(doc: &str, part: usize, text: &str) -> KbHit {
    KbHit {
        chunk_id: "c".into(),
        document_id: "d".into(),
        document_name: doc.into(),
        collection_id: "k".into(),
        collection_name: "K".into(),
        part,
        text: text.into(),
        score: 0.1,
    }
}

// ------------------------------------------------------------------ chunker

#[test]
fn empty_and_blank_texts_have_no_passages() {
    assert!(chunk_text("").is_empty());
    assert!(chunk_text("  \n\n \r\n  ").is_empty());
}

#[test]
fn a_short_text_is_one_passage_and_paragraphs_are_packed_up_to_the_target() {
    assert_eq!(chunk_text("Una riga sola."), ["Una riga sola."]);
    let para = |c: char| c.to_string().repeat(500);
    let text = format!("{}\n\n{}\n\n{}", para('a'), para('b'), para('c'));
    let chunks = chunk_with(&text, 1_200, 0);
    // 500 + 2 + 500 fits, the third paragraph starts a new passage.
    assert_eq!(chunks.len(), 2, "{chunks:?}");
    assert_eq!(chunks[0], format!("{}\n\n{}", para('a'), para('b')));
    assert_eq!(chunks[1], para('c'));
}

#[test]
fn a_heading_starts_a_new_passage_and_stays_with_its_paragraph() {
    let text = "# Uno\n\nPrimo testo.\n\n## Due\nSecondo testo.\n\nAncora secondo.";
    let chunks = chunk_with(text, 1_200, 0);
    assert_eq!(chunks, ["# Uno\n\nPrimo testo.", "## Due\nSecondo testo.\n\nAncora secondo."]);
    assert!(!chunk_with("#nonintestazione\ntesto", 1_200, 0).is_empty());
    assert_eq!(chunk_with("#nonintestazione\n\ntesto", 1_200, 0).len(), 1, "a # without a space is not a heading");
}

#[test]
fn every_passage_after_the_first_begins_with_the_end_of_the_previous_one() {
    let words: Vec<String> = (0..400).map(|i| format!("parola{i}")).collect();
    let text = words.join(" ");
    let chunks = chunk_with(&text, 300, 60);
    assert!(chunks.len() > 3);
    for pair in chunks.windows(2) {
        let first_word_of_next = pair[1].split_whitespace().next().unwrap();
        assert!(pair[0].contains(first_word_of_next), "{first_word_of_next} is repeated from the previous passage");
        // The overlap starts at a word, not in the middle of one.
        assert!(pair[0].split_whitespace().any(|w| w == first_word_of_next), "whole word: {first_word_of_next}");
    }
    assert!(chunks.iter().all(|c| c.chars().count() <= 300 + 60 + 2), "bounded");
    // Nothing is lost: every word is in at least one passage.
    let joined = chunks.join(" ");
    assert!(words.iter().all(|w| joined.contains(w.as_str())));
    // No overlap: no repeated words.
    let plain = chunk_with(&text, 300, 0);
    assert_eq!(plain.join(" ").split_whitespace().count(), 400);
}

#[test]
fn chunking_never_splits_inside_a_character_and_cuts_text_without_spaces() {
    let accented = "è".repeat(5_000);
    let chunks = chunk_with(&accented, 1_200, 150);
    assert!(chunks.len() >= 4);
    assert!(chunks.iter().all(|c| c.chars().all(|ch| ch == 'è' || ch == '\n') && c.chars().count() <= 1_352));
    let emoji = "😀日本語 ".repeat(800);
    let chunks = chunk_text(&emoji);
    assert!(chunks.len() > 1 && chunks.iter().all(|c| !c.is_empty()));
    let crlf = chunk_with("Riga uno.\r\n\r\nRiga due.", 1_200, 0);
    assert_eq!(crlf, ["Riga uno.\n\nRiga due."]);
    // A long paragraph is cut at a sentence end when there is one.
    let sentences = "Questa è una frase abbastanza lunga. ".repeat(20);
    let cut = chunk_with(&sentences, 200, 0);
    assert!(cut.iter().all(|c| c.ends_with('.')), "{cut:?}");
}

#[test]
fn chunking_is_deterministic() {
    let text = format!("# T\n\n{}", "Testo di prova con più parole. ".repeat(200));
    assert_eq!(chunk_text(&text), chunk_text(&text));
}

// ------------------------------------------------------------------ ranking

#[test]
fn top_k_keeps_the_best_in_order_and_matches_the_plain_ranking() {
    let stored = vec![
        ("z".to_string(), vec![1.0, 0.0]),
        ("a".to_string(), vec![1.0, 0.0]),
        ("m".to_string(), vec![1.0, 1.0]),
        ("far".to_string(), vec![0.0, 1.0]),
        ("w".to_string(), vec![2.0, 1.0]),
    ];
    let q = [1.0, 0.0];
    let plain = semantic_rank(&q, &stored);
    let mut top = TopK::new(10);
    for (id, v) in &stored {
        top.push(id, crate::memory::cosine(&q, v));
    }
    let all = top.finish();
    assert_eq!(all.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>(), plain.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>());
    // A smaller n keeps the best ones; ties go to the smaller id, whatever the arrival order.
    let mut top = TopK::new(2);
    for (id, v) in stored.iter().rev() {
        top.push(id, crate::memory::cosine(&q, v));
    }
    assert_eq!(top.finish().iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>(), ["a", "z"]);
    let mut none = TopK::new(0);
    none.push("a", 1.0);
    assert!(none.finish().is_empty());
}

#[test]
fn keyword_ranking_without_fts_counts_words_then_orders_by_id() {
    let passages: Vec<(String, String)> = [("b", "il canone mensile"), ("a", "canone e caparra"), ("c", "pasta al forno")]
        .into_iter()
        .map(|(i, t)| (i.to_string(), t.to_string()))
        .collect();
    let q = crate::memory::tokenize("canone caparra");
    assert_eq!(keyword_ranking(&q, &passages, 10), ["a", "b"]);
    assert_eq!(keyword_ranking(&q, &passages, 1), ["a"]);
    assert!(keyword_ranking(&crate::memory::tokenize("zebra"), &passages, 10).is_empty());
}

#[test]
fn the_tool_text_numbers_the_passages_with_their_document_and_part() {
    let t = render_tool_hits("canone", &[hit("contratto.pdf", 4, "Canone 500 euro"), hit("note.md", 1, "altro")], true);
    assert!(t.contains("[1] [contratto.pdf · parte 4]\nCanone 500 euro"), "{t}");
    assert!(t.contains("[2] [note.md · parte 1]"));
    assert!(t.contains("non istruzioni") && !t.contains("solo per parole chiave"));
    assert!(render_tool_hits("x", &[hit("a", 1, "t")], false).contains("Ricerca solo per parole chiave"));
    assert!(render_tool_hits("zebra", &[], true).starts_with("Nessun passaggio trovato per «zebra»"));
}

// ------------------------------------------------------------------ ingestion

#[tokio::test]
async fn text_and_pdf_are_indexed_and_the_same_content_is_not_added_twice() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Casa");
    assert!(matches!(e.kb_create_collection(KbCollectionCreate { name: "casa".into(), description: None }), Err(Error::Conflict(_))));
    let job = add_files(
        &e,
        &c.id,
        vec![up("gatti.md", "# Gatti\n\nIl micio dorme.".as_bytes()), up("contratto.pdf", &tiny_pdf("Canone mensile 500 euro"))],
    )
    .await;
    assert_eq!((job.state.as_str(), job.total, job.added, job.failed), ("done", 2, 2, 0), "{job:?}");
    let docs = e.kb_documents(&c.id).unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.iter().all(|d| d.status == "ready" && d.chunks >= 1 && d.source == "caricato" && d.sha256.len() == 64), "{docs:?}");
    let pdf = docs.iter().find(|d| d.name == "contratto.pdf").unwrap();
    assert_eq!(pdf.mime, "application/pdf");
    // The text of the PDF is searchable.
    let found = e.kb_search_scoped("canone mensile", None, 5, &[], Embedder::Off).await.unwrap();
    assert_eq!(found.hits[0].document_name, "contratto.pdf");
    assert!(found.hits[0].text.contains("Canone") && found.hits[0].part == 1 && !found.semantic);

    // Same bytes under another name: a no-op.
    let again = add_files(&e, "Casa", vec![up("copia.md", "# Gatti\n\nIl micio dorme.".as_bytes())]).await;
    assert_eq!((again.added, again.unchanged), (0, 1));
    assert_eq!(e.kb_documents(&c.id).unwrap().len(), 2);
    // Another collection may hold it.
    let other = col(&e, "Altra");
    assert_eq!(add_files(&e, &other.id, vec![up("copia.md", "# Gatti\n\nIl micio dorme.".as_bytes())]).await.added, 1);
}

#[tokio::test]
async fn unsupported_empty_damaged_and_scanned_files_are_reported_and_the_rest_goes_on() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Misto");
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    let job = add_files(
        &e,
        &c.id,
        vec![
            up("foto.png", &png),
            up("vuoto.txt", b""),
            up("binario.bin", &[0, 1, 2, 3, 255]),
            up("scansione.pdf", &tiny_pdf("")),
            up("rotto.pdf", b"%PDF-1.4 non e' un pdf"),
            up("buono.txt", b"Questo file va bene."),
        ],
    )
    .await;
    assert_eq!((job.added, job.failed), (1, 2), "{job:?}");
    let names: Vec<&str> = job.skipped.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"foto.png") && names.contains(&"vuoto.txt") && names.contains(&"binario.bin"), "{names:?}");
    assert!(job.skipped.iter().find(|s| s.name == "foto.png").unwrap().reason.contains("non supportato"));
    let docs = e.kb_documents(&c.id).unwrap();
    let bad: Vec<_> = docs.iter().filter(|d| d.status == "failed").collect();
    assert_eq!(bad.len(), 2);
    assert!(bad.iter().all(|d| d.error.as_deref().is_some_and(|m| !m.is_empty())));
    assert_eq!(docs.iter().filter(|d| d.status == "ready").count(), 1);
    // Retrying a failed file replaces the failed row instead of stacking another one.
    add_files(&e, &c.id, vec![up("rotto.pdf", b"%PDF-1.4 non e' un pdf")]).await;
    assert_eq!(e.kb_documents(&c.id).unwrap().len(), 3);
}

#[tokio::test]
async fn upload_limits_are_the_chat_attachment_limits_with_their_own_words() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Limiti");
    let add = |files: Vec<AttachmentUpload>| e.kb_add(&c.id, KbAddBody { files: Some(files), path: None }, true);
    let many: Vec<_> = (0..9).map(|i| up(&format!("{i}.txt"), b"x y z")).collect();
    let err = add(many).await.unwrap_err();
    assert!(matches!(&err, Error::Invalid(m) if m.contains("al massimo 8 file per volta")), "{err}");
    let big = vec![b'a'; 10 * 1024 * 1024 + 1];
    let err = add(vec![up("grande.txt", &big)]).await.unwrap_err();
    assert!(matches!(&err, Error::Invalid(m) if m.contains("«grande.txt» è troppo grande")), "{err}");
    let err = add(vec![AttachmentUpload { name: "a.txt".into(), mime: String::new(), data: "!!!".into() }]).await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)));
    assert!(matches!(add(vec![]).await, Err(Error::Invalid(_))));
    assert!(matches!(e.kb_add(&c.id, KbAddBody::default(), true).await, Err(Error::Invalid(_))));
    assert!(matches!(
        e.kb_add("nessuna", KbAddBody { files: Some(vec![up("a.txt", b"ciao")]), path: None }, true).await,
        Err(Error::NotFound(_))
    ));
    let both = KbAddBody { files: Some(vec![up("a.txt", b"ciao")]), path: Some("/tmp".into()) };
    assert!(matches!(e.kb_add(&c.id, both, true).await, Err(Error::Invalid(_))));
}

#[tokio::test]
async fn a_folder_is_walked_a_changed_file_replaces_its_document_and_skips_are_reported() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Cartella");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("sotto")).unwrap();
    std::fs::write(root.join("a.md"), "# A\n\nTesto di a sul gatto.").unwrap();
    std::fs::write(root.join("sotto/b.rs"), "fn principale() { /* codice */ }").unwrap();
    std::fs::write(root.join("immagine.png"), [0x89, b'P', b'N', b'G', 0, 0, 0, 0]).unwrap();
    std::fs::write(root.join(".nascosto.txt"), "non va").unwrap();
    std::fs::write(root.join("enorme.txt"), vec![b'x'; 10 * 1024 * 1024 + 1]).unwrap();

    let job = add_path(&e, &c.id, root).await;
    assert_eq!((job.total, job.added, job.failed), (2, 2, 0), "{job:?}");
    let skipped: Vec<(&str, &str)> = job.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect();
    assert!(skipped.iter().any(|(n, r)| *n == "immagine.png" && r.contains("non supportato")), "{skipped:?}");
    assert!(skipped.iter().any(|(n, r)| *n == "enorme.txt" && r.contains("10 MB")), "{skipped:?}");
    assert!(!skipped.iter().any(|(n, _)| n.contains("nascosto")), "hidden files are not even listed");
    let docs = e.kb_documents(&c.id).unwrap();
    assert_eq!(docs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["a.md", "sotto/b.rs"]);
    let canonical = root.canonicalize().unwrap().join("a.md").to_string_lossy().into_owned();
    assert_eq!(docs[0].source, canonical);

    // Nothing changed: all unchanged.
    let same = add_path(&e, &c.id, root).await;
    assert_eq!((same.added, same.unchanged, same.replaced), (0, 2, 0));
    // A changed file at the same path replaces the old document (and its passages).
    let old_id = docs[0].id.clone();
    std::fs::write(root.join("a.md"), "# A\n\nTesto nuovo sulla pasta.").unwrap();
    let changed = add_path(&e, &c.id, &root.join("a.md")).await;
    assert_eq!((changed.added, changed.replaced), (0, 1), "{changed:?}");
    let docs = e.kb_documents(&c.id).unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.iter().all(|d| d.id != old_id));
    let fresh = e.kb_search_scoped("pasta", None, 5, &[], Embedder::Off).await.unwrap();
    assert_eq!(fresh.hits.len(), 1);
    assert!(e.kb_search_scoped("gatto", None, 5, &[], Embedder::Off).await.unwrap().hits.is_empty(), "the old text is gone");

    // A path that is not there or is not absolute is a clear error.
    let err = e.kb_add(&c.id, KbAddBody { files: None, path: Some(root.join("non-esiste-proprio").to_string_lossy().into_owned()) }, true).await.unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("non esiste")), "{err}");
    assert!(matches!(e.kb_add(&c.id, KbAddBody { files: None, path: Some("relativo/x".into()) }, true).await, Err(Error::Invalid(_))));
    // A caller that must not read this computer's files is refused.
    let denied = e.kb_add(&c.id, KbAddBody { files: None, path: Some(root.to_string_lossy().into_owned()) }, false).await.unwrap_err();
    assert!(matches!(denied, Error::Denied(_)));

    // Reindex reads the files again: the changed ones are replaced, a missing one is reported.
    std::fs::remove_file(root.join("sotto/b.rs")).unwrap();
    std::fs::write(root.join("a.md"), "# A\n\nAncora un altro testo.").unwrap();
    let r = e.kb_reindex(&c.id).unwrap();
    e.kb_wait_idle().await;
    let job = e.kb_job(&r.job.id).unwrap();
    assert_eq!((job.replaced, job.unchanged), (1, 0), "{job:?}");
    assert!(job.skipped.iter().any(|s| s.name == "sotto/b.rs" && s.reason.contains("non c'è più")), "{job:?}");
    assert_eq!(e.kb_documents(&c.id).unwrap().len(), 2, "the missing file's document stays");
}

#[test]
fn a_folder_is_cut_at_the_file_limit_and_the_rest_is_counted() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(dir.path().join(format!("f{i}.txt")), "ciao mondo").unwrap();
    }
    let (files, skipped, total) = walk_with(dir.path(), 3);
    assert_eq!(files.len(), 3);
    assert_eq!((skipped.len(), total), (2, 2));
    assert!(skipped[0].reason.contains("oltre il limite"), "{skipped:?}");
    assert_eq!(MAX_FILES, 2_000);
}

#[tokio::test]
async fn a_job_can_be_cancelled_and_nothing_is_left_half_done() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Annulla");
    let files: Vec<_> = (0..8).map(|i| up(&format!("n{i}.txt"), format!("nota numero {i}").as_bytes())).collect();
    let r = e.kb_add(&c.id, KbAddBody { files: Some(files), path: None }, true).await.unwrap();
    assert_eq!((r.job.state.as_str(), r.job.total), ("running", 8));
    e.kb_cancel(&r.job.id).unwrap();
    e.kb_wait_idle().await;
    let job = e.kb_job(&r.job.id).unwrap();
    assert_eq!(job.state, "cancelled");
    assert!(job.done < 8);
    assert!(e.kb_documents(&c.id).unwrap().iter().all(|d| d.status == "ready"), "no document is left indexing");
    assert!(matches!(e.kb_cancel("kbj_nessuno"), Err(Error::NotFound(_))));
    assert_eq!(e.kb_status().jobs[0].id, r.job.id);
}

// ------------------------------------------------------------------ search

#[tokio::test]
async fn without_an_embedding_model_search_is_by_keywords_and_says_so() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Parole");
    add_files(
        &e,
        &c.id,
        vec![up("gatti.md", "I gatti dormono molto.".as_bytes()), up("caffe.txt", "Il caffè è una bevanda calda.".as_bytes())],
    )
    .await;
    assert!(e.embedding_model_name().is_none());
    let found = e.kb_search("gatt", None, 5).await.unwrap();
    assert!(!found.semantic);
    assert_eq!(found.hits.iter().map(|h| h.document_name.as_str()).collect::<Vec<_>>(), ["gatti.md"]);
    // Accents are folded, a question with only stop words finds nothing, an empty one is refused.
    assert_eq!(e.kb_search("caffe", None, 5).await.unwrap().hits.len(), 1);
    assert!(e.kb_search("il la di", None, 5).await.unwrap().hits.is_empty());
    assert!(matches!(e.kb_search("  ", None, 5).await, Err(Error::Invalid(_))));
    // Nothing was waiting to be embedded in the background and nothing failed.
    assert_eq!(e.embed_pending(Embedder::Auto).await.unwrap(), 0);
    assert_eq!(e.kb_collection(&c.id).unwrap().embedded_chunks, 0);
}

#[tokio::test]
async fn embeddings_are_computed_in_the_background_and_fused_with_keywords() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Temi");
    add_files(
        &e,
        &c.id,
        vec![
            up("gatti.md", "Il micio dorme tutto il giorno.".as_bytes()),
            up("caffe.txt", "L'espresso si beve caldo.".as_bytes()),
            up("lavoro.txt", "Riunione in ufficio alle nove.".as_bytes()),
        ],
    )
    .await;
    // Meaning: «gatto» shares no word with «micio» but the vectors agree; no vectors yet = keywords only.
    assert!(e.kb_search_scoped("il mio gatto", None, 5, &[], themes()).await.unwrap().hits.is_empty());
    assert_eq!(e.embed_pending(themes()).await.unwrap(), 3);
    assert_eq!(e.embed_pending(themes()).await.unwrap(), 0, "nothing left");
    let col = e.kb_collection(&c.id).unwrap();
    assert_eq!((col.chunks, col.embedded_chunks), (3, 3));
    let found = e.kb_search_scoped("il mio gatto", None, 5, &[], themes()).await.unwrap();
    assert!(found.semantic);
    assert_eq!(found.hits.iter().map(|h| h.document_name.as_str()).collect::<Vec<_>>(), ["gatti.md"]);
    // Both rankings agree on a passage: it has the fused score of two first places.
    let both = e.kb_search_scoped("micio", None, 5, &[], themes()).await.unwrap();
    assert_eq!(both.hits[0].document_name, "gatti.md");
    assert!((both.hits[0].score - 2.0 / 61.0).abs() < 1e-9, "{}", both.hits[0].score);
    // A new document waits for its vector; deleting a document removes its vector with it.
    let docs = e.kb_documents(&c.id).unwrap();
    e.kb_delete_document(&docs[0].id).unwrap();
    assert_eq!(e.kb_collection(&c.id).unwrap().embedded_chunks, 2);
}

#[tokio::test]
async fn the_provider_a_shell_brings_is_used_by_the_background_pass_and_by_search() {
    let (e, _d, _w) = engine(vec![]).await;
    let c = col(&e, "Provider");
    add_files(&e, &c.id, vec![up("gatti.md", "Il micio dorme.".as_bytes()), up("caffe.txt", "Un espresso.".as_bytes())]).await;
    e.use_embedder(Some(Arc::new(Themes)), "temi");
    assert_eq!(e.embedding_model_name().as_deref(), Some("temi"));
    assert_eq!(e.kb_status().embedding_model.as_deref(), Some("temi"));
    // The pass the job started in the background finishes on its own.
    for _ in 0..200 {
        if e.kb_collection(&c.id).unwrap().embedded_chunks == 2 {
            break;
        }
        e.kb_embed_now().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(e.kb_collection(&c.id).unwrap().embedded_chunks, 2);
    let found = e.kb_search("gatto", None, 3).await.unwrap();
    assert!(found.semantic && found.hits[0].document_name == "gatti.md");
    // Reindex forgets the vectors and the pass computes them again.
    e.kb_reindex(&c.id).unwrap();
    assert_eq!(e.kb_collection(&c.id).unwrap().embedded_chunks, 0);
    for _ in 0..200 {
        e.kb_embed_now().await.unwrap();
        if e.kb_collection(&c.id).unwrap().embedded_chunks == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(e.kb_collection(&c.id).unwrap().embedded_chunks, 2);
    e.use_embedder(None, "");
    assert!(e.embedding_model_name().is_none());
}

#[tokio::test]
async fn search_can_be_limited_to_a_collection_by_name_or_id() {
    let (e, _d, _w) = engine(vec![]).await;
    let (a, b) = (col(&e, "Alfa"), col(&e, "Beta"));
    add_files(&e, &a.id, vec![up("uno.txt", b"la parola comune e alfa")]).await;
    add_files(&e, &b.id, vec![up("due.txt", b"la parola comune e beta")]).await;
    let all = e.kb_search("comune", None, 10).await.unwrap();
    assert_eq!(all.hits.len(), 2);
    for key in [a.id.as_str(), "alfa", " ALFA "] {
        let r = e.kb_search("comune", Some(key), 10).await.unwrap();
        assert_eq!(r.hits.iter().map(|h| h.document_name.as_str()).collect::<Vec<_>>(), ["uno.txt"], "{key}");
        assert_eq!(r.hits[0].collection_name, "Alfa");
    }
    assert!(matches!(e.kb_search("comune", Some("nessuna"), 10).await, Err(Error::NotFound(_))));
    assert_eq!(e.kb_search("comune", None, 1).await.unwrap().hits.len(), 1);
    assert_eq!(e.kb_search("comune", None, 1_000).await.unwrap().hits.len(), 2, "capped at 10 and there are two");
}

// ------------------------------------------------------------------ deletion

#[tokio::test]
async fn deleting_a_document_or_a_collection_removes_its_passages_from_search() {
    let (e, _d, _w) = engine(vec![]).await;
    let (a, b) = (col(&e, "Alfa"), col(&e, "Beta"));
    add_files(&e, &a.id, vec![up("uno.txt", b"il canone di affitto"), up("due.txt", b"la caparra")]).await;
    add_files(&e, &b.id, vec![up("tre.txt", b"la pasta al forno")]).await;
    assert_eq!(e.0.store.kb_chunk_count().unwrap(), 3);
    let uno = e.kb_documents(&a.id).unwrap().into_iter().find(|d| d.name == "uno.txt").unwrap();
    e.kb_delete_document(&uno.id).unwrap();
    assert!(e.kb_search("canone", None, 5).await.unwrap().hits.is_empty());
    assert_eq!(e.0.store.kb_chunk_count().unwrap(), 2);
    assert!(matches!(e.kb_delete_document(&uno.id), Err(Error::NotFound(_))));
    e.kb_delete_collection("Alfa").unwrap();
    assert!(e.kb_search("caparra", None, 5).await.unwrap().hits.is_empty());
    assert_eq!(e.0.store.kb_chunk_count().unwrap(), 1);
    assert_eq!(e.kb_collections().unwrap().len(), 1);
    assert!(matches!(e.kb_collection(&a.id), Err(Error::NotFound(_))));
    assert_eq!(e.kb_search("pasta", None, 5).await.unwrap().hits.len(), 1);
    // Renaming keeps the documents, a taken name is a conflict.
    let r = e
        .kb_update_collection(&b.id, aiw_core::api::KbCollectionPatch { name: Some("Ricette".into()), description: Some("di casa".into()) })
        .unwrap();
    assert_eq!((r.name.as_str(), r.description.as_str(), r.documents), ("Ricette", "di casa", 1));
    let other = col(&e, "Altra");
    let taken = e.kb_update_collection(&other.id, aiw_core::api::KbCollectionPatch { name: Some("ricette".into()), description: None });
    assert!(matches!(taken, Err(Error::Conflict(_))));
    assert!(matches!(e.kb_create_collection(KbCollectionCreate { name: "  ".into(), description: None }), Err(Error::Invalid(_))));
    assert!(matches!(e.kb_create_collection(KbCollectionCreate { name: "x".repeat(81), description: None }), Err(Error::Invalid(_))));
}

// ------------------------------------------------------------------ the agent tool

#[tokio::test]
async fn the_tool_is_offered_only_with_a_collection_and_scoped_by_the_agents_list() {
    let (e, _d, _w) = engine(vec![]).await;
    assert!(!e.kb_available(&[]));
    // Empty base: a note in Italian, not an error.
    let note = e.kb_tool(&[], "kb_search", &json!({"query": "qualcosa"})).await.unwrap();
    assert!(note.starts_with("La base di conoscenza è vuota"), "{note}");
    let (a, b) = (col(&e, "Alfa"), col(&e, "Beta"));
    assert!(e.kb_available(&[]) && e.kb_available(std::slice::from_ref(&a.id)));
    assert!(!e.kb_available(&["kbk_sparita".to_string()]), "a list of deleted collections never means «all»");
    assert!(e.kb_tool(&[], "kb_search", &json!({"query": "x"})).await.unwrap().contains("vuota"), "collections without documents");
    add_files(&e, &a.id, vec![up("uno.txt", b"il canone di affitto mensile")]).await;
    add_files(&e, &b.id, vec![up("due.txt", b"il canone del cellulare")]).await;

    let all = e.kb_tool(&[], "kb_search", &json!({"query": "canone"})).await.unwrap();
    assert!(all.contains("[uno.txt · parte 1]") && all.contains("[due.txt · parte 1]"), "{all}");
    assert!(all.contains("[1] ") && all.contains("[2] ") && all.contains("Ricerca solo per parole chiave"));
    // The agent that lists Alfa only never sees Beta.
    let scoped = e.kb_tool(std::slice::from_ref(&a.id), "kb_search", &json!({"query": "canone"})).await.unwrap();
    assert!(scoped.contains("uno.txt") && !scoped.contains("due.txt"), "{scoped}");
    let denied = e.kb_tool(std::slice::from_ref(&a.id), "kb_search", &json!({"query": "canone", "collection": "Beta"})).await.unwrap_err();
    assert!(matches!(denied, Error::Denied(_)), "{denied}");
    let named = e.kb_tool(&[], "kb_search", &json!({"query": "canone", "collection": "beta", "max_results": 1})).await.unwrap();
    assert!(named.contains("due.txt") && !named.contains("uno.txt"));
    assert!(e.kb_tool(&[], "kb_search", &json!({})).await.unwrap_err().to_string().contains("`query` mancante"));
    assert!(matches!(e.kb_tool(&[], "altro", &json!({"query": "x"})).await, Err(Error::Invalid(_))));
    let none = e.kb_tool(&[], "kb_search", &json!({"query": "zebra"})).await.unwrap();
    assert!(none.starts_with("Nessun passaggio trovato"), "{none}");
    // With the meaning of the question too.
    let sem = e.kb_tool_with(&[], &json!({"query": "il canone"}), themes()).await.unwrap();
    assert!(!sem.contains("Ricerca solo per parole chiave") || sem.contains("uno.txt"));
}

#[tokio::test]
async fn an_agent_with_the_tool_gets_the_prompt_line_and_can_search_in_a_run() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "kb_search", json!({"query": "canone"})),
        ScriptedBackend::text_turn("Il canone è 500 euro [contratto.txt]"),
    ];
    let backend = ScriptedBackend::new(turns);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    // No collection: no tool, no prompt line.
    let s0 = e.create_session("operator", None).unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn("ok")])));
    let probe = ScriptedBackend::new(vec![ScriptedBackend::text_turn("ok")]);
    e.register_native_runtime("llama-embedded", Arc::new(probe.clone()));
    let run = e.start_run(&s0.id, "ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    {
        let req = &probe.requests.lock().unwrap()[0];
        assert!(!req.tools.iter().any(|t| t.function.name == "kb_search"), "no collection: no tool");
        assert!(!req.messages[0].text().contains("Documenti a disposizione"));
    }

    let c = col(&e, "Contratti");
    add_files(&e, &c.id, vec![up("contratto.txt", b"Il canone mensile e di 500 euro.")]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    // `ask` mode: a registry tool would wait for an approval; the search must not.
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Quanto è il canone?".into()).await.unwrap();
    wait_finished(&e, &run).await;
    assert!(e.approvals().is_empty());
    let reqs = backend.requests.lock().unwrap();
    assert!(reqs[0].tools.iter().any(|t| t.function.name == "kb_search"));
    let system = reqs[0].messages[0].text();
    assert!(system.contains("# Documenti a disposizione") && system.contains("«Contratti»") && system.contains("`kb_search`"), "{system}");
    assert!(!system.contains("500 euro"), "passages are never injected into the prompt");
    let last = reqs.last().unwrap().messages.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
    assert!(last.contains("[contratto.txt · parte 1]") && last.contains("500 euro"), "{last}");
    drop(reqs);
    assert!(e.audit().into_iter().any(|a| a.action == "tool" && a.tool.as_deref() == Some("kb_search")));
}

#[tokio::test]
async fn an_agents_knowledge_list_limits_the_prompt_and_is_validated() {
    let (e, _d, _w) = engine(vec![]).await;
    let (a, b) = (col(&e, "Alfa"), col(&e, "Beta"));
    let base = e.agent("operator").unwrap();
    assert!(base.tools.iter().any(|t| t == "kb_search"), "new installs get the tool");
    assert!(e.agent("planner").unwrap().tools.iter().any(|t| t == "kb_search"));
    assert!(!e.agent("coder").unwrap().tools.iter().any(|t| t == "kb_search"), "«*» agents get it without a list");
    let line = e.kb_prompt(&base).unwrap();
    assert!(line.contains("«Alfa»") && line.contains("«Beta»"), "{line}");
    let scoped = AgentProfile { id: "solo-alfa".into(), name: "Solo Alfa".into(), knowledge: vec![a.id.clone()], ..base.clone() };
    let saved = e.save_agent(scoped.clone()).unwrap();
    assert_eq!(saved.knowledge, [a.id.as_str()]);
    let line = e.kb_prompt(&saved).unwrap();
    assert!(line.contains("«Alfa»") && !line.contains("«Beta»"), "{line}");
    let invalid = e.save_agent(AgentProfile { knowledge: vec!["kbk_nessuna".into()], ..scoped.clone() });
    assert!(matches!(&invalid, Err(Error::Invalid(m)) if m.contains("kbk_nessuna")), "{invalid:?}");
    // An agent that does not list the tool gets no line; a deleted collection leaves nothing to say.
    let no_tool = AgentProfile { tools: vec!["read_file".into()], ..base.clone() };
    assert!(e.kb_prompt(&no_tool).is_none());
    e.kb_delete_collection(&a.id).unwrap();
    assert!(e.kb_prompt(&saved).is_none(), "its only collection is gone: not «all»");
    assert!(e.kb_prompt(&base).unwrap().contains("«Beta»"));
    let _ = b;
    // The agent JSON keeps working without the field.
    let json = serde_json::to_value(&base).unwrap();
    assert!(json.get("knowledge").is_none());
    let back: AgentProfile = serde_json::from_value(json).unwrap();
    assert!(back.knowledge.is_empty());
}

#[tokio::test]
async fn the_proactive_read_only_run_may_search_documents() {
    let (e, _d, _w) = engine(vec![]).await;
    let a = e.proactive_agent("operator").unwrap();
    assert!(a.tools.iter().any(|t| t == "kb_search") && a.mode == "plan");
}
