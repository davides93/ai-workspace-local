use crate::{arg_str, truncate_middle, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(ReadFile), Arc::new(WriteFile), Arc::new(EditFile), Arc::new(ListDir), Arc::new(Glob), Arc::new(Grep), Arc::new(MoveFile)]
}

const MAX_READ: usize = 120_000;

/// Workspace-relative path shown to the model, always with `/` so that prompts, tool results and
/// tests read the same on Windows.
fn rel(ctx: &ToolContext, p: &Path) -> String {
    match p.strip_prefix(&ctx.workspace_root) {
        Ok(r) if r.as_os_str().is_empty() => ".".into(),
        Ok(r) => slash_path(r),
        Err(_) => p.display().to_string(),
    }
}

fn slash_path(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

struct ReadFile;
#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Legge un file di testo e restituisce le righe numerate. Usa offset/limit per i file lunghi."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string","description":"Percorso relativo al workspace o assoluto"},
            "offset":{"type":"integer","description":"Prima riga (1 = inizio)"},
            "limit":{"type":"integer","description":"Numero massimo di righe (default 400)"}
        },"required":["path"]})
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let p = ctx.resolve(arg_str(&args, "path")?, false)?;
        let size = tokio::fs::metadata(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?.len();
        // Read at most the first MAX_READ_BYTES: a multi-GB log must not be loaded in memory.
        let mut bytes = Vec::new();
        {
            use tokio::io::AsyncReadExt;
            let f = tokio::fs::File::open(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?;
            f.take(MAX_READ_BYTES).read_to_end(&mut bytes).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?;
        }
        if bytes.iter().take(8000).any(|b| *b == 0) {
            return Ok(ToolOutput::text(format!("{} è un file binario ({} byte).", rel(ctx, &p), bytes.len())));
        }
        ctx.mark_seen(&p);
        let text = String::from_utf8_lossy(&bytes);
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(400) as usize;
        let total = text.lines().count();
        let partial = size > MAX_READ_BYTES;
        let mut out = String::new();
        for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
            out.push_str(&format!("{:>6}\t{}\n", i + 1, line));
        }
        if offset - 1 + limit < total {
            out.push_str(&format!("… ({} righe totali; continua con offset={})\n", total, offset + limit));
        }
        if partial {
            out.push_str(&format!("… file di {} MB: lette solo le prime {} MB\n", size >> 20, MAX_READ_BYTES >> 20));
        }
        Ok(ToolOutput::text(truncate_middle(&out, MAX_READ)))
    }
}

/// Upper bound of what `read_file` loads from disk.
const MAX_READ_BYTES: u64 = 16 << 20;

struct WriteFile;
#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Crea o sovrascrive un file di testo (le cartelle mancanti vengono create). Con append=true aggiunge content in fondo al file esistente."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},
            "content":{"type":"string"},
            "append":{"type":"boolean","description":"true = aggiunge in fondo invece di sostituire il file"}
        },"required":["path","content"]})
    }
    fn risk(&self) -> Risk {
        Risk::Write
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let p = ctx.resolve(arg_str(&args, "path")?, true)?;
        let content = arg_str(&args, "content")?;
        let append = args.get("append").and_then(|v| v.as_bool()).unwrap_or(false);
        let existed = p.exists();
        if existed && !append && ctx.require_read_before_overwrite && !ctx.was_seen(&p) {
            return Ok(ToolOutput::error(format!(
                "{} esiste già e non l'hai ancora letto: per aggiungere testo in fondo richiama write_file con append=true; per cambiarlo leggilo con read_file e usa edit_file.",
                rel(ctx, &p)
            )));
        }
        ctx.checkpoint(&p);
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if append && existed {
            let mut current = tokio::fs::read_to_string(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?;
            if !current.is_empty() && !current.ends_with('\n') && !content.starts_with('\n') {
                current.push('\n');
            }
            current.push_str(content);
            tokio::fs::write(&p, &current).await?;
        } else {
            tokio::fs::write(&p, content).await?;
        }
        ctx.mark_seen(&p);
        if !existed {
            ctx.record_created(&p);
        }
        Ok(ToolOutput::text(format!(
            "{} {} ({} righe, {} byte)",
            match (existed, append) {
                (true, true) => "Aggiunto in fondo a",
                (true, false) => "Sovrascritto",
                _ => "Creato",
            },
            rel(ctx, &p),
            content.lines().count(),
            content.len()
        )))
    }
}

struct EditFile;
#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Sostituisce un testo esatto in un file. old_string deve comparire una sola volta, salvo replace_all=true. Con old_string vuoto aggiunge new_string in fondo al file (o lo crea)."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "path":{"type":"string"},
            "old_string":{"type":"string","description":"Testo esistente, copiato esattamente"},
            "new_string":{"type":"string"},
            "replace_all":{"type":"boolean"}
        },"required":["path","old_string","new_string"]})
    }
    fn risk(&self) -> Risk {
        Risk::Write
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let p = ctx.resolve(arg_str(&args, "path")?, true)?;
        let old = arg_str(&args, "old_string")?;
        let new = arg_str(&args, "new_string")?;
        if old.is_empty() {
            // Small models use an empty old_string to append: do exactly that.
            let existed = p.exists();
            ctx.checkpoint(&p);
            let mut text = if existed {
                tokio::fs::read_to_string(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?
            } else {
                if let Some(parent) = p.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                String::new()
            };
            if !text.is_empty() && !text.ends_with('\n') && !new.starts_with('\n') {
                text.push('\n');
            }
            text.push_str(new);
            tokio::fs::write(&p, &text).await?;
            ctx.mark_seen(&p);
            if !existed {
                ctx.record_created(&p);
            }
            return Ok(ToolOutput::text(format!(
                "{} {} ({} righe aggiunte)",
                if existed { "Aggiunto in fondo a" } else { "Creato" },
                rel(ctx, &p),
                new.lines().count()
            )));
        }
        let all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
        let text = tokio::fs::read_to_string(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?;
        let n = text.matches(old).count();
        if n == 0 {
            return Ok(ToolOutput::error(format!("old_string non trovato in {}. Rileggi il file e copia il testo esatto.", rel(ctx, &p))));
        }
        if n > 1 && !all {
            return Ok(ToolOutput::error(format!(
                "old_string compare {n} volte in {}: aggiungi contesto o usa replace_all=true.",
                rel(ctx, &p)
            )));
        }
        ctx.checkpoint(&p);
        let updated = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
        tokio::fs::write(&p, &updated).await?;
        ctx.mark_seen(&p);
        let line = text.find(old).map(|i| text[..i].lines().count() + 1).unwrap_or(1);
        Ok(ToolOutput::text(format!(
            "Modificato {} ({} sostituzion{}, dalla riga {line})",
            rel(ctx, &p),
            if all { n } else { 1 },
            if all && n > 1 { "i" } else { "e" }
        )))
    }
}

struct ListDir;
#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "Elenca il contenuto di una cartella (nome, tipo, dimensione, data). pattern filtra per nome, es. *.pdf."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string","description":"Cartella (default: radice del workspace)"},"pattern":{"type":"string"}}})
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let p = ctx.resolve(args.get("path").and_then(|v| v.as_str()).unwrap_or("."), false)?;
        let matcher = match args.get("pattern").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            Some(pat) => Some(globset::Glob::new(pat).map_err(|e| Error::Invalid(e.to_string()))?.compile_matcher()),
            None => None,
        };
        let mut rd = tokio::fs::read_dir(&p).await.map_err(|e| Error::Invalid(format!("{}: {e}", p.display())))?;
        let mut rows = Vec::new();
        while let Some(e) = rd.next_entry().await? {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(m) = &matcher {
                if !m.is_match(&name) {
                    continue;
                }
            }
            let meta = e.metadata().await.ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            rows.push((is_dir, name, size, modified));
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
        let total = rows.len();
        let mut out = format!("{} ({} elementi)\n", rel(ctx, &p), total);
        for (is_dir, name, size, modified) in rows.into_iter().take(500) {
            if is_dir {
                out.push_str(&format!("{name}/\n"));
            } else {
                out.push_str(&format!("{name}\t{}\t{modified}\n", human(size)));
            }
        }
        if total > 500 {
            out.push_str("… (mostrati i primi 500)\n");
        }
        Ok(ToolOutput::text(out))
    }
}

pub fn human(n: u64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut x = n as f64;
    let mut i = 0;
    while x >= 1024.0 && i < u.len() - 1 {
        x /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{x:.1} {}", u[i])
    }
}

fn walker(root: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(root).hidden(true).git_ignore(true).max_depth(Some(24)).build()
}

struct Glob;
#[async_trait]
impl Tool for Glob {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Trova file per pattern (es. **/*.rs, docs/**/*.md), rispettando .gitignore."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string","description":"Cartella di partenza"}},"required":["pattern"]})
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let root = ctx.resolve(args.get("path").and_then(|v| v.as_str()).unwrap_or("."), false)?;
        let pat = arg_str(&args, "pattern")?.to_string();
        let m = globset::Glob::new(&pat).map_err(|e| Error::Invalid(e.to_string()))?.compile_matcher();
        let ws = ctx.workspace_root.clone();
        let out = tokio::task::spawn_blocking(move || {
            let mut hits = Vec::new();
            for e in walker(&root).flatten() {
                if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                let r = e.path().strip_prefix(&root).unwrap_or(e.path());
                if m.is_match(r) || m.is_match(e.file_name()) {
                    hits.push(slash_path(e.path().strip_prefix(&ws).unwrap_or(e.path())));
                    if hits.len() >= 500 {
                        break;
                    }
                }
            }
            hits
        })
        .await
        .map_err(Error::other)?;
        Ok(ToolOutput::text(if out.is_empty() {
            format!("Nessun file corrisponde a {pat}")
        } else {
            format!("{} file\n{}", out.len(), out.join("\n"))
        }))
    }
}

struct Grep;
#[async_trait]
impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Cerca una regex nei file di testo (file:riga: testo). Usa glob per filtrare i file."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "pattern":{"type":"string","description":"Espressione regolare"},
            "path":{"type":"string"},
            "glob":{"type":"string","description":"Filtro sui file, es. *.ts"},
            "case_insensitive":{"type":"boolean"}
        },"required":["pattern"]})
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let root = ctx.resolve(args.get("path").and_then(|v| v.as_str()).unwrap_or("."), false)?;
        let ci = args.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
        let re =
            regex::RegexBuilder::new(arg_str(&args, "pattern")?).case_insensitive(ci).build().map_err(|e| Error::Invalid(e.to_string()))?;
        let filter = match args.get("glob").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            Some(g) => Some(globset::Glob::new(g).map_err(|e| Error::Invalid(e.to_string()))?.compile_matcher()),
            None => None,
        };
        let ws = ctx.workspace_root.clone();
        let out = tokio::task::spawn_blocking(move || {
            let mut hits = Vec::new();
            'files: for e in walker(&root).flatten() {
                if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    continue;
                }
                if let Some(f) = &filter {
                    if !f.is_match(e.file_name()) {
                        continue;
                    }
                }
                if e.metadata().map(|m| m.len() > 4 << 20).unwrap_or(true) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(e.path()) else { continue };
                if bytes.iter().take(4000).any(|b| *b == 0) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                let shown = slash_path(e.path().strip_prefix(&ws).unwrap_or(e.path()));
                for (i, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        let l: String = line.chars().take(240).collect();
                        hits.push(format!("{shown}:{}: {l}", i + 1));
                        if hits.len() >= 200 {
                            break 'files;
                        }
                    }
                }
            }
            hits
        })
        .await
        .map_err(Error::other)?;
        Ok(ToolOutput::text(if out.is_empty() { "Nessuna corrispondenza".into() } else { out.join("\n") }))
    }
}

struct MoveFile;
#[async_trait]
impl Tool for MoveFile {
    fn name(&self) -> &str {
        "move_file"
    }
    fn description(&self) -> &str {
        "Sposta o rinomina un file o una cartella dentro il workspace (crea la cartella di destinazione)."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"]})
    }
    fn risk(&self) -> Risk {
        Risk::Write
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let from = ctx.resolve(arg_str(&args, "from")?, true)?;
        let mut to = ctx.resolve(arg_str(&args, "to")?, true)?;
        if to.is_dir() {
            to = to.join(from.file_name().ok_or_else(|| Error::Invalid("origine non valida".into()))?);
        }
        if to.exists() {
            return Ok(ToolOutput::error(format!("{} esiste già: scegli un altro nome", rel(ctx, &to))));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // The original of the source is kept; the destination did not exist, so a rewind deletes it
        // (a moved folder is not tracked: only files are).
        ctx.checkpoint(&from);
        let is_file = from.is_file();
        tokio::fs::rename(&from, &to).await.map_err(|e| Error::Invalid(format!("spostamento non riuscito: {e}")))?;
        if is_file {
            ctx.record_created(&to);
        }
        Ok(ToolOutput::text(format!("{} → {}", rel(ctx, &from), rel(ctx, &to))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint;

    #[tokio::test]
    async fn write_edit_read_list_grep_move() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("run_t", dir.path().to_path_buf());
        ctx.checkpoint_dir = Some(dir.path().join(".ckpt"));
        let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();

        let out = t("write_file").call(json!({"path":"notes/a.md","content":"ciao mondo\nseconda riga\n"}), &ctx).await.unwrap();
        assert!(out.content.contains("Creato notes/a.md"));
        let out = t("edit_file").call(json!({"path":"notes/a.md","old_string":"mondo","new_string":"workspace"}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        // The file was created by this run: a rewind deletes it, so no "original" is kept.
        assert!(!dir.path().join(".ckpt/run_t/notes/a.md").exists(), "no original for a file the run created");
        let out = t("read_file").call(json!({"path":"notes/a.md"}), &ctx).await.unwrap();
        assert!(out.content.contains("ciao workspace"));
        let out = t("edit_file").call(json!({"path":"notes/a.md","old_string":"assente","new_string":"x"}), &ctx).await.unwrap();
        assert!(out.is_error);
        let out = t("list_dir").call(json!({"path":"notes","pattern":"*.md"}), &ctx).await.unwrap();
        assert!(out.content.contains("a.md"));
        let out = t("grep").call(json!({"pattern":"seconda"}), &ctx).await.unwrap();
        assert!(out.content.contains("notes/a.md:2"), "{}", out.content);
        let out = t("glob").call(json!({"pattern":"**/*.md"}), &ctx).await.unwrap();
        assert!(out.content.contains("notes/a.md"));
        let out = t("move_file").call(json!({"from":"notes/a.md","to":"archivio/2026/a.md"}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        assert!(dir.path().join("archivio/2026/a.md").exists());
        assert!(t("write_file").call(json!({"path":"../evil.txt","content":"x"}), &ctx).await.is_err());
    }

    #[tokio::test]
    async fn file_tools_feed_the_checkpoint_of_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let ckpt = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("run_c", dir.path().to_path_buf());
        ctx.checkpoint_dir = Some(ckpt.path().to_path_buf());
        let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();
        std::fs::write(dir.path().join("vecchio.md"), "originale").unwrap();
        std::fs::write(dir.path().join("da-spostare.md"), "contenuto").unwrap();

        t("write_file").call(json!({"path":"nuovo.md","content":"uno"}), &ctx).await.unwrap();
        t("write_file").call(json!({"path":"nuovo.md","content":"due"}), &ctx).await.unwrap();
        t("write_file").call(json!({"path":"vecchio.md","content":"sostituito"}), &ctx).await.unwrap();
        t("edit_file").call(json!({"path":"creato-da-edit.md","old_string":"","new_string":"x"}), &ctx).await.unwrap();
        t("move_file").call(json!({"from":"da-spostare.md","to":"archivio/spostato.md"}), &ctx).await.unwrap();

        let info = checkpoint::plan(ckpt.path(), "run_c").unwrap();
        assert_eq!(info.restore, vec!["da-spostare.md", "vecchio.md"]);
        assert_eq!(info.delete, vec!["archivio/spostato.md", "creato-da-edit.md", "nuovo.md"]);
        // `nuovo.md` was created by the run: overwriting it again must not make "uno" look like an original.
        assert!(!ckpt.path().join("run_c/nuovo.md").exists());
        let r = checkpoint::rewind(ckpt.path(), "run_c").unwrap();
        assert!(r.failed.is_empty());
        assert_eq!(std::fs::read_to_string(dir.path().join("vecchio.md")).unwrap(), "originale");
        assert_eq!(std::fs::read_to_string(dir.path().join("da-spostare.md")).unwrap(), "contenuto");
        assert!(!dir.path().join("nuovo.md").exists() && !dir.path().join("archivio/spostato.md").exists());
    }

    /// Symlinks cannot be created without privileges on Windows, hence unix only.
    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_cannot_leave_the_workspace() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "segreto fuori").unwrap();
        let ws = tempfile::tempdir().unwrap();
        let w = ws.path();
        symlink(outside.path(), w.join("link_dir")).unwrap();
        symlink(&secret, w.join("link_file")).unwrap();
        symlink(outside.path().join("nuovo.txt"), w.join("dangling")).unwrap();
        std::fs::create_dir(w.join("sub")).unwrap();
        symlink("../..", w.join("sub/up")).unwrap(); // relative link climbing out of the workspace
        std::fs::write(w.join("dentro.txt"), "ok dentro").unwrap();
        symlink(w.join("dentro.txt"), w.join("alias_interno")).unwrap();

        let ctx = ToolContext::new("run_t", w.to_path_buf());
        let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();
        let denied = |r: Result<ToolOutput>, what: &str| match r {
            Err(Error::Denied(m)) => assert!(m.contains("collegamento simbolico"), "{what}: {m}"),
            other => panic!("{what}: expected a denial, got {other:?}"),
        };

        // Read.
        denied(t("read_file").call(json!({"path": "link_file"}), &ctx).await, "read through a file link");
        denied(t("read_file").call(json!({"path": "link_dir/secret.txt"}), &ctx).await, "read through a directory link");
        denied(t("read_file").call(json!({"path": "sub/up/secret.txt"}), &ctx).await, "read through a relative link");
        denied(t("read_file").call(json!({"path": "dangling"}), &ctx).await, "read a dangling link");
        denied(t("list_dir").call(json!({"path": "link_dir"}), &ctx).await, "list through a link");
        denied(t("glob").call(json!({"pattern": "*", "path": "link_dir"}), &ctx).await, "glob in a linked folder");
        denied(t("grep").call(json!({"pattern": "segreto", "path": "link_dir"}), &ctx).await, "grep in a linked folder");

        // Write: the outside files stay as they were and nothing is created there.
        denied(t("write_file").call(json!({"path": "link_dir/pwned.txt", "content": "x"}), &ctx).await, "create through a directory link");
        denied(t("write_file").call(json!({"path": "link_dir/a/b/pwned.txt", "content": "x"}), &ctx).await, "create deep through a link");
        denied(t("write_file").call(json!({"path": "link_file", "content": "x"}), &ctx).await, "overwrite through a file link");
        denied(
            t("write_file").call(json!({"path": "link_file", "content": "x", "append": true}), &ctx).await,
            "append through a file link",
        );
        denied(t("write_file").call(json!({"path": "dangling", "content": "x"}), &ctx).await, "create through a dangling link");
        denied(
            t("edit_file").call(json!({"path": "link_file", "old_string": "segreto", "new_string": "x"}), &ctx).await,
            "edit through a link",
        );
        denied(
            t("edit_file").call(json!({"path": "link_dir/nuovo2.txt", "old_string": "", "new_string": "x"}), &ctx).await,
            "append-create through a link",
        );
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "segreto fuori");
        let left: Vec<_> =
            std::fs::read_dir(outside.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left, vec!["secret.txt"], "nothing was created outside");

        // Move: neither as source nor as destination.
        denied(t("move_file").call(json!({"from": "link_file", "to": "copia.txt"}), &ctx).await, "move from a link");
        denied(t("move_file").call(json!({"from": "dentro.txt", "to": "link_dir/dentro.txt"}), &ctx).await, "move into a linked folder");
        assert!(w.join("dentro.txt").exists() && !outside.path().join("dentro.txt").exists());

        // Searching from the workspace root does not walk into linked folders.
        let g = t("grep").call(json!({"pattern": "segreto"}), &ctx).await.unwrap();
        assert!(!g.content.contains("segreto"), "{}", g.content);
        let g = t("glob").call(json!({"pattern": "**/*.txt"}), &ctx).await.unwrap();
        assert!(g.content.contains("dentro.txt") && !g.content.contains("secret"), "{}", g.content);

        // A link that stays inside the workspace is fine, for reading and writing.
        let r = t("read_file").call(json!({"path": "alias_interno"}), &ctx).await.unwrap();
        assert!(r.content.contains("ok dentro"), "{}", r.content);
        let r = t("edit_file").call(json!({"path": "alias_interno", "old_string": "ok", "new_string": "OK"}), &ctx).await.unwrap();
        assert!(!r.is_error, "{}", r.content);
        assert_eq!(std::fs::read_to_string(w.join("dentro.txt")).unwrap(), "OK dentro");
        // New files are judged by the folder they are created in.
        let r = t("write_file").call(json!({"path": "nuova/cartella/file.txt", "content": "x"}), &ctx).await.unwrap();
        assert!(!r.is_error && w.join("nuova/cartella/file.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_roots_are_readable_through_links_but_never_writable() {
        use std::os::unix::fs::symlink;
        let skills = tempfile::tempdir().unwrap();
        std::fs::write(skills.path().join("SKILL.md"), "istruzioni").unwrap();
        let ws = tempfile::tempdir().unwrap();
        symlink(skills.path(), ws.path().join("skills")).unwrap();
        let mut ctx = ToolContext::new("run_t", ws.path().to_path_buf());
        ctx.read_roots = vec![skills.path().to_path_buf()];
        let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();
        let r = t("read_file").call(json!({"path": "skills/SKILL.md"}), &ctx).await.unwrap();
        assert!(r.content.contains("istruzioni"), "{}", r.content);
        assert!(t("write_file").call(json!({"path": "skills/SKILL.md", "content": "x", "append": true}), &ctx).await.is_err());
        assert_eq!(std::fs::read_to_string(skills.path().join("SKILL.md")).unwrap(), "istruzioni");
    }

    #[tokio::test]
    async fn agents_read_before_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "riga 1\n").unwrap();
        let mut ctx = ToolContext::new("run_t", dir.path().to_path_buf());
        ctx.require_read_before_overwrite = true;
        let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();

        let out = t("write_file").call(json!({"path":"note.md","content":"solo questa"}), &ctx).await.unwrap();
        assert!(out.is_error && out.content.contains("read_file"), "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("note.md")).unwrap(), "riga 1\n");
        t("read_file").call(json!({"path":"./note.md"}), &ctx).await.unwrap();
        let out = t("write_file").call(json!({"path":"note.md","content":"riga 1\nriga 2\n"}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        let out = t("edit_file").call(json!({"path":"note.md","old_string":"","new_string":"riga 2.5\n"}), &ctx).await.unwrap();
        assert!(out.content.starts_with("Aggiunto in fondo"), "{}", out.content);
        let out = t("write_file").call(json!({"path":"note.md","content":"riga 3","append":true}), &ctx).await.unwrap();
        assert!(out.content.starts_with("Aggiunto in fondo"), "{}", out.content);
        assert_eq!(std::fs::read_to_string(dir.path().join("note.md")).unwrap(), "riga 1\nriga 2\nriga 2.5\nriga 3");
        // New files, and files this run already wrote, need no read.
        for content in ["a", "b"] {
            let out = t("write_file").call(json!({"path":"nuovo.md","content":content}), &ctx).await.unwrap();
            assert!(!out.is_error, "{}", out.content);
        }
    }
}
