//! Attachments of the user messages: kinds, limits, storage, what the model receives and the history rule.

use super::attachments::*;
use super::tests::{engine, wait_finished};
use super::*;
use aiw_core::api::{Attachment, AttachmentKind, AttachmentUpload};
use aiw_runtimes::{ContentPart, MsgContent, ScriptedBackend};
use base64::Engine as _;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn upload(name: &str, mime: &str, bytes: &[u8]) -> AttachmentUpload {
    AttachmentUpload { name: name.into(), mime: mime.into(), data: b64(bytes) }
}

/// A one-page PDF with a line of text, with correct cross-reference offsets.
pub(crate) fn tiny_pdf(text: &str) -> Vec<u8> {
    let stream = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![];
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
    out
}

fn att(id: &str, name: &str, mime: &str, size: u64, kind: AttachmentKind) -> Attachment {
    Attachment { id: id.into(), name: name.into(), mime: mime.into(), size, kind }
}

fn file(a: Attachment, text: Option<&str>, image: bool) -> AttFile {
    let path = PathBuf::from(format!("/data/attachments/s/{}-{}", a.id, a.name));
    let image = image.then(|| format!("data:{};base64,AAAA", a.mime));
    AttFile { att: a, readable_path: Some(path.clone()), path: Some(path), text: text.map(String::from), image }
}

fn user(content: &str, atts: Vec<Attachment>) -> ChatMessage {
    ChatMessage {
        id: new_id("msg"),
        role: "user".into(),
        content: content.into(),
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        attachments: (!atts.is_empty()).then_some(atts),
        created_at: now_rfc3339(),
    }
}

// ------------------------------------------------------------------ pure code

#[test]
fn names_are_made_safe_and_keep_their_extension() {
    assert_eq!(sanitize_name("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_name("C:\\Users\\me\\nota.txt"), "nota.txt");
    assert_eq!(sanitize_name(".hidden"), "hidden");
    assert_eq!(sanitize_name("a\u{0}b\nc.txt"), "abc.txt");
    assert_eq!(sanitize_name(""), "allegato");
    assert_eq!(sanitize_name("../"), "allegato");
    let long = format!("{}.pdf", "è".repeat(300));
    let n = sanitize_name(&long);
    assert!(n.len() <= 120 && n.ends_with(".pdf"), "{n}");
}

#[test]
fn kinds_come_from_the_content_before_the_declared_type() {
    assert_eq!(detect("foto.bin", "application/octet-stream", PNG), (AttachmentKind::Image, "image/png".into()));
    assert_eq!(detect("x.png", "image/png", b"\xFF\xD8\xFFrest").0, AttachmentKind::Image);
    assert_eq!(detect("x", "", b"GIF89a....").1, "image/gif");
    assert_eq!(detect("x", "", b"RIFF\0\0\0\0WEBPVP8 ").1, "image/webp");
    assert_eq!(detect("n.md", "", "# Ciao\nè".as_bytes()), (AttachmentKind::Text, "text/markdown".into()));
    assert_eq!(detect("dati.csv", "text/csv", b"a,b\n1,2").0, AttachmentKind::Text);
    assert_eq!(detect("senza", "", b"solo testo").1, "text/plain");
    assert_eq!(detect("r.pdf", "", b"%PDF-1.7 ...").0, AttachmentKind::Document);
    // binary, NUL bytes, invalid UTF-8, empty, unsupported image formats: plain files
    assert_eq!(detect("a.bin", "", &[0, 1, 2, 3]).0, AttachmentKind::File);
    assert_eq!(detect("a.txt", "", &[0xFF, 0xFE, 0x41]).0, AttachmentKind::File);
    assert_eq!(detect("vuoto.txt", "text/plain", b"").0, AttachmentKind::File);
    assert_eq!(detect("logo.svg", "image/svg+xml", b"<svg/>").0, AttachmentKind::File);
    assert_eq!(detect("a.zip", "application/zip", b"PK\x03\x04\0").0, AttachmentKind::File);
    // an invented mime type is not trusted blindly
    assert_eq!(detect("a.bin", "text/<script>", &[0, 1]).1, "application/octet-stream");
}

#[test]
fn limits_are_checked_with_italian_messages() {
    let err = |u: Vec<AttachmentUpload>| match prepare(u) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("accepted"),
    };
    let nine: Vec<_> = (0..9).map(|i| upload(&format!("{i}.txt"), "", b"x")).collect();
    assert!(err(nine).contains("al massimo 8 file"));
    let big = vec![b'a'; (10 << 20) + 1];
    assert!(err(vec![upload("grande.txt", "", &big)]).contains("«grande.txt» è troppo grande"));
    let nine_mb = vec![b'a'; 9 << 20];
    let total: Vec<_> = (0..3).map(|i| upload(&format!("{i}.txt"), "", &nine_mb)).collect();
    assert!(err(total).contains("in totale al massimo 25 MB"));
    assert!(err(vec![AttachmentUpload { name: "x".into(), mime: String::new(), data: "!!!".into() }]).contains("non è leggibile"));
    let ok = prepare(vec![upload("../a.txt", "", b"ciao"), upload("b.png", "", PNG)]).unwrap();
    assert_eq!((ok[0].name.as_str(), ok[0].kind), ("a.txt", AttachmentKind::Text));
    assert_eq!(ok[1].kind, AttachmentKind::Image);
}

#[test]
fn text_and_documents_are_inlined_in_blocks() {
    let files = vec![
        file(att("att_1", "nota.txt", "text/plain", 5, AttachmentKind::Text), Some("ciao"), false),
        file(att("att_2", "r.pdf", "application/pdf", 900, AttachmentKind::Document), Some("testo del pdf"), false),
    ];
    let r = render_attachments(&files, true, true);
    assert_eq!(r.text, "\n\n[Allegato: nota.txt]\nciao\n[Fine allegato]\n\n[Allegato: r.pdf]\ntesto del pdf\n[Fine allegato]");
    assert!(r.images.is_empty() && !r.vision_blocked);
}

#[test]
fn a_long_text_is_clipped_and_names_where_to_read_it() {
    let long = "Q".repeat(TEXT_BUDGET + 500);
    let files = vec![file(att("att_1", "grande.txt", "text/plain", long.len() as u64, AttachmentKind::Text), Some(&long), false)];
    let r = render_attachments(&files, true, true);
    assert_eq!(r.text.matches('Q').count(), TEXT_BUDGET);
    assert!(r.text.contains("troncato") && r.text.contains("/data/attachments/s/att_1-grande.txt"), "{}", r.text);
    assert!(r.text.ends_with("[Fine allegato]"));
}

#[test]
fn the_total_budget_of_a_message_is_shared_and_the_rest_becomes_a_note() {
    let chunk = "y".repeat(TEXT_BUDGET);
    let files: Vec<AttFile> = (0..5)
        .map(|i| file(att(&format!("att_{i}"), &format!("f{i}.txt"), "text/plain", 100, AttachmentKind::Text), Some(&chunk), false))
        .collect();
    let r = render_attachments(&files, true, true);
    assert_eq!(r.text.matches('y').count(), TOTAL_TEXT_BUDGET);
    assert!(r.text.contains("[Allegato: f3.txt] Contenuto non incluso"), "{}", r.text);
    assert!(r.text.contains("/data/attachments/s/att_4-f4.txt"));
}

#[test]
fn other_files_are_only_described_with_their_path() {
    let files = vec![
        file(att("att_1", "dati.zip", "application/zip", 2048, AttachmentKind::File), None, false),
        file(att("att_2", "scan.pdf", "application/pdf", 1_500_000, AttachmentKind::File), None, false),
    ];
    let r = render_attachments(&files, true, true);
    assert!(r.text.contains("[Allegato: dati.zip] File allegato (application/zip, 2,0 KB), non incluso nel messaggio."), "{}", r.text);
    assert!(r.text.contains("/data/attachments/s/att_1-dati.zip"));
    assert!(r.text.contains("scansione") && r.text.contains("1,4 MB"));
}

#[test]
fn images_become_parts_unless_the_model_cannot_see_them() {
    let files = vec![file(att("att_1", "foto.png", "image/png", 10, AttachmentKind::Image), None, true)];
    let r = render_attachments(&files, true, true);
    assert_eq!(r.images, vec!["data:image/png;base64,AAAA".to_string()]);
    assert_eq!(r.text, "\n\n[Immagine allegata: foto.png]");
    let blind = render_attachments(&files, true, false);
    assert!(blind.images.is_empty() && blind.vision_blocked);
    assert!(blind.text.contains("non può vedere le immagini"));
}

#[test]
fn a_missing_file_is_a_note_and_never_an_error() {
    let mut f = file(att("att_1", "sparito.txt", "text/plain", 5, AttachmentKind::Text), None, false);
    f.path = None;
    let r = render_attachments(&[f], true, true);
    assert_eq!(r.text, "\n\n[Allegato: sparito.txt] Il file non è più disponibile.");
}

#[test]
fn history_keeps_the_text_of_every_attachment_and_the_images_of_the_last_two_messages() {
    let img = |i: usize| att(&format!("att_{i}"), &format!("foto{i}.png"), "image/png", 10, AttachmentKind::Image);
    let stored = vec![
        user("uno", vec![img(1)]),
        user("due", vec![img(2)]),
        user("tre", vec![img(3)]),
        user("quattro", vec![att("att_9", "n.txt", "text/plain", 3, AttachmentKind::Text)]),
        user("solo testo", vec![]),
    ];
    let mut asked = vec![];
    let msgs = build_history(&stored, true, &mut |m, want| {
        asked.push((m.content.clone(), want));
        m.attachments
            .as_ref()
            .unwrap()
            .iter()
            .map(|a| file(a.clone(), (a.kind == AttachmentKind::Text).then_some("contenuto"), want && a.kind == AttachmentKind::Image))
            .collect()
    });
    assert_eq!(msgs.len(), 5);
    assert_eq!(asked.iter().map(|(_, w)| *w).collect::<Vec<_>>(), vec![false, true, true, false]);
    // the oldest image is a note, the recent ones are parts
    assert!(matches!(&msgs[0].content, Some(MsgContent::Text(t)) if t.contains("Non è più mostrata") && t.starts_with("uno")));
    assert!(matches!(&msgs[1].content, Some(MsgContent::Parts(p)) if p.len() == 2));
    assert!(matches!(&msgs[2].content, Some(MsgContent::Parts(p)) if p.len() == 2));
    assert!(matches!(&msgs[3].content, Some(MsgContent::Text(t)) if t.contains("[Allegato: n.txt]\ncontenuto\n[Fine allegato]")));
    assert_eq!(msgs[4].text(), "solo testo");
}

#[test]
fn history_skips_blocked_prompts_and_notes() {
    let mut blocked = user("vietato", vec![att("att_1", "a.png", "image/png", 1, AttachmentKind::Image)]);
    blocked.name = Some(BLOCKED_PROMPT_MESSAGE.into());
    let msgs = build_history(&[blocked, user("ok", vec![])], true, &mut |_, _| panic!("must not be loaded"));
    assert_eq!(msgs.len(), 1);
}

// ------------------------------------------------------------------ engine

async fn attach_engine(caps: Option<Vec<&str>>) -> (Engine, ScriptedBackend, tempfile::TempDir, tempfile::TempDir) {
    let (e, d, w) = engine(vec![]).await;
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("visto"), ScriptedBackend::text_turn("ancora")]);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    if let Some(caps) = caps {
        *e.0.models.write().unwrap() = vec![LocalModel {
            id: "scripted".into(),
            name: "Scripted".into(),
            capabilities: caps.into_iter().map(String::from).collect(),
            ..Default::default()
        }];
    }
    (e, backend, d, w)
}

/// The messages of the `n`-th request the backend received (the lock is not kept).
fn request_messages(b: &ScriptedBackend, n: usize) -> Vec<aiw_runtimes::Msg> {
    b.requests.lock().unwrap()[n].messages.clone()
}

fn custom(e: &Engine, run: &str, name: &str) -> Vec<Value> {
    e.run_events(run)
        .iter()
        .filter_map(|s| match serde_json::to_value(&s.event.event).unwrap() {
            v if v["type"] == "CUSTOM" && v["name"] == name => Some(v["value"].clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_run_stores_the_attachments_and_the_model_receives_them() {
    let (e, backend, d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e
        .start_run_attach(
            &s.id,
            "Che cosa dice?".into(),
            None,
            None,
            vec![upload("../nota.txt", "text/plain", "Il gatto è sul tavolo".as_bytes()), upload("foto.png", "", PNG)],
        )
        .await
        .unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs[0].content, "Che cosa dice?", "the stored text is what was typed");
    let atts = msgs[0].attachments.clone().unwrap();
    assert_eq!(atts.len(), 2);
    assert_eq!((atts[0].name.as_str(), atts[0].kind, atts[0].size), ("nota.txt", AttachmentKind::Text, 22));
    assert_eq!((atts[1].mime.as_str(), atts[1].kind), ("image/png", AttachmentKind::Image));
    let json = serde_json::to_string(&msgs[0]).unwrap();
    assert!(!json.contains(d.path().to_str().unwrap()), "no absolute path in the message: {json}");
    // on disk
    let dir = d.path().join("attachments").join(&s.id);
    assert!(dir.join(format!("{}-nota.txt", atts[0].id)).is_file());
    // what the model got
    let sent = request_messages(&backend, 0);
    let last = sent.last().unwrap();
    let Some(MsgContent::Parts(parts)) = &last.content else { panic!("{:?}", last.content) };
    let ContentPart::Text { text } = &parts[0] else { panic!() };
    assert!(text.starts_with("Che cosa dice?\n\n[Allegato: nota.txt]\nIl gatto è sul tavolo\n[Fine allegato]"), "{text}");
    assert!(text.contains("[Immagine allegata: foto.png]"));
    assert!(matches!(&parts[1], ContentPart::ImageUrl { image_url } if image_url.url.starts_with("data:image/png;base64,")));
    // the agent may read the folder (and only that) next to the workspace
    let input = e.build_input_at(
        "r",
        &s.id,
        e.agent("operator").unwrap(),
        e.profile("default").unwrap(),
        vec![],
        String::new(),
        CancellationToken::new(),
        0,
        Default::default(),
        None,
    );
    assert!(input.tool_ctx.read_roots.contains(&dir));
    // the data comes back for the UI
    let data = e.attachment_data(&s.id, &atts[0].id).unwrap();
    assert_eq!((data.name.as_str(), data.mime.as_str()), ("nota.txt", "text/plain"));
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(data.data).unwrap(), "Il gatto è sul tavolo".as_bytes());
    assert!(matches!(e.attachment_data(&s.id, "att_nope"), Err(Error::NotFound(_))));
    assert!(matches!(e.attachment_data(&s.id, "../../x"), Err(Error::NotFound(_))));
    // a follow-up keeps the text of the file in the history
    let run2 = e.start_run(&s.id, "E poi?".into()).await.unwrap();
    wait_finished(&e, &run2).await;
    let hist: Vec<String> = request_messages(&backend, 1).iter().map(|m| m.text()).collect();
    assert!(hist.iter().any(|t| t.contains("Il gatto è sul tavolo")), "{hist:?}");
}

#[tokio::test]
async fn an_attachment_alone_is_a_message_and_names_the_session() {
    let (e, backend, _d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_attach(&s.id, "  ".into(), None, None, vec![upload("bilancio.csv", "", b"a;b\n1;2")]).await.unwrap();
    wait_finished(&e, &run).await;
    assert_eq!(e.session(&s.id).unwrap().title, "bilancio.csv");
    assert!(request_messages(&backend, 0).last().unwrap().text().contains("[Allegato: bilancio.csv]"));
    // neither text nor files is still empty
    let err = e.start_run_attach(&s.id, "".into(), None, None, vec![]).await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)));
}

#[tokio::test]
async fn a_model_without_vision_gets_a_note_and_a_warning_event() {
    let (e, backend, _d, _w) = attach_engine(Some(vec!["chat"])).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_attach(&s.id, "Guarda".into(), None, None, vec![upload("foto.png", "", PNG)]).await.unwrap();
    wait_finished(&e, &run).await;
    let sent = request_messages(&backend, 0);
    let last = sent.last().unwrap();
    assert!(matches!(&last.content, Some(MsgContent::Text(t)) if t.contains("non può vedere le immagini")), "{:?}", last.content);
    let ev = custom(&e, &run, "aiw.attachments");
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["warning"], NO_VISION_WARNING);
    // a model with vision gets the image and no warning
    let (e, backend, _d, _w) = attach_engine(Some(vec!["chat", "vision"])).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_attach(&s.id, "Guarda".into(), None, None, vec![upload("foto.png", "", PNG)]).await.unwrap();
    wait_finished(&e, &run).await;
    assert!(matches!(&request_messages(&backend, 0).last().unwrap().content, Some(MsgContent::Parts(_))));
    assert!(custom(&e, &run, "aiw.attachments").is_empty());
}

#[tokio::test]
async fn too_many_or_too_large_attachments_are_refused_before_anything_is_stored() {
    let (e, _b, d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let nine: Vec<_> = (0..9).map(|i| upload(&format!("{i}.txt"), "", b"x")).collect();
    assert!(matches!(e.start_run_attach(&s.id, "ciao".into(), None, None, nine).await, Err(Error::Invalid(_))));
    assert!(e.messages(&s.id).unwrap().is_empty());
    assert!(!d.path().join("attachments").join(&s.id).exists());
}

#[tokio::test]
async fn deleting_the_session_deletes_its_attachments() {
    let (e, _b, d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_attach(&s.id, "ciao".into(), None, None, vec![upload("a.txt", "", b"uno")]).await.unwrap();
    wait_finished(&e, &run).await;
    let dir = d.path().join("attachments").join(&s.id);
    assert!(dir.is_dir());
    e.delete_session(&s.id).unwrap();
    assert!(!dir.exists());
}

#[tokio::test]
async fn a_deleted_file_is_a_note_in_the_next_turn() {
    let (e, backend, d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_attach(&s.id, "ciao".into(), None, None, vec![upload("a.txt", "", b"uno")]).await.unwrap();
    wait_finished(&e, &run).await;
    std::fs::remove_dir_all(d.path().join("attachments")).unwrap();
    let run2 = e.start_run(&s.id, "ancora".into()).await.unwrap();
    wait_finished(&e, &run2).await;
    assert!(request_messages(&backend, 1).iter().any(|m| m.text().contains("[Allegato: a.txt] Il file non è più disponibile.")));
}

#[tokio::test]
async fn a_pdf_keeps_its_text_next_to_the_original() {
    let (e, backend, d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e
        .start_run_attach(
            &s.id,
            "Riassumi".into(),
            None,
            None,
            vec![upload("contratto.pdf", "application/pdf", &tiny_pdf("Canone mensile 500 euro"))],
        )
        .await
        .unwrap();
    wait_finished(&e, &run).await;
    let a = e.messages(&s.id).unwrap()[0].attachments.clone().unwrap().remove(0);
    assert_eq!(a.kind, AttachmentKind::Document, "text could be extracted");
    let side = d.path().join("attachments").join(&s.id).join(format!("{}.extracted.txt", a.id));
    assert!(std::fs::read_to_string(side).unwrap().contains("Canone mensile 500 euro"));
    assert!(request_messages(&backend, 0).last().unwrap().text().contains("Canone mensile 500 euro"));
}

#[tokio::test]
async fn a_pdf_without_text_or_a_damaged_one_is_a_plain_file() {
    let (e, backend, _d, _w) = attach_engine(None).await;
    let s = e.create_session("operator", None).unwrap();
    let uploads = vec![upload("scan.pdf", "", &tiny_pdf("")), upload("rotto.pdf", "", b"%PDF-1.4 non e' un pdf")];
    let run = e.start_run_attach(&s.id, "ciao".into(), None, None, uploads).await.unwrap();
    wait_finished(&e, &run).await;
    let atts = e.messages(&s.id).unwrap()[0].attachments.clone().unwrap();
    assert!(atts.iter().all(|a| a.kind == AttachmentKind::File), "{atts:?}");
    let text = request_messages(&backend, 0).last().unwrap().text();
    assert!(text.contains("PDF senza testo leggibile"), "{text}");
}
