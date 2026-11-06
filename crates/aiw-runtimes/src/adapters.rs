//! LoRA adapters in inference profiles: which runtimes take them, what the files must look like
//! and the launch arguments they become.
//!
//! The profile names adapters by path (`InferenceParams.adapters`); the registry of the adapters AIW
//! trained (`Engine::adapters`) provides the paths. A runtime that cannot load an adapter refuses
//! the profile when it is saved, not at the first message.

use crate::manifest::{AdapterSupport, RuntimeManifest};
use aiw_core::api::AdapterRef;
use aiw_core::{Error, Result};
use std::path::{Path, PathBuf};

/// Files that make a folder an MLX adapter (`mlx_lm.lora` writes both).
const MLX_FILES: [&str; 2] = ["adapters.safetensors", "adapter_config.json"];

/// The path of an adapter as the runtime gets it (`~` expanded).
pub fn resolve(path: &str) -> PathBuf {
    aiw_core::paths::expand_home(path.trim())
}

/// What is wrong with the adapter at `path` for `format`, or `Ok`.
pub fn check_files(path: &Path, format: &str) -> std::result::Result<(), String> {
    let shown = path.display();
    match format {
        "gguf" => {
            if path.is_dir() {
                let peft = path.join("adapter_config.json").is_file();
                return Err(if peft {
                    format!(
                        "{shown} è un adapter PEFT, ma llama.cpp vuole un file GGUF: convertilo con convert_lora_to_gguf.py di llama.cpp"
                    )
                } else {
                    format!("{shown} è una cartella: llama.cpp vuole un file GGUF")
                });
            }
            if !path.is_file() {
                return Err(format!("{shown} non esiste"));
            }
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gguf")) {
                return Err(format!("{shown} non è un file GGUF (estensione .gguf)"));
            }
            Ok(())
        }
        "mlx" | "peft" => {
            if path.is_file() {
                return Err(format!(
                    "{shown} è un file: serve la cartella dell'adapter ({})",
                    if format == "mlx" { MLX_FILES.join(" e ") } else { "adapter_config.json e adapter_model.safetensors".into() }
                ));
            }
            if !path.is_dir() {
                return Err(format!("{shown} non esiste"));
            }
            let missing: Vec<&str> = if format == "mlx" {
                MLX_FILES.iter().copied().filter(|f| !path.join(f).is_file()).collect()
            } else {
                let weights = path.join("adapter_model.safetensors").is_file() || path.join("adapter_model.bin").is_file();
                let mut m = Vec::new();
                if !path.join("adapter_config.json").is_file() {
                    m.push("adapter_config.json");
                }
                if !weights {
                    m.push("adapter_model.safetensors");
                }
                m
            };
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("{shown} non è un adapter {}: mancano {}", format.to_uppercase(), missing.join(" e ")))
            }
        }
        other => Err(format!("formato di adapter sconosciuto nel manifest: {other}")),
    }
}

/// Checks that `manifest` can load `adapters` and that each one is what it expects.
pub fn validate(manifest: &RuntimeManifest, adapters: &[AdapterRef]) -> Result<()> {
    if adapters.is_empty() {
        return Ok(());
    }
    let Some(support) = &manifest.adapters else {
        return Err(Error::Unsupported(match &manifest.adapter_hint {
            Some(hint) => format!("{} non carica adapter LoRA da un profilo. {hint}", manifest.name),
            None => format!("{} non carica adapter LoRA da un profilo", manifest.name),
        }));
    };
    if adapters.len() > support.max {
        return Err(Error::Invalid(if support.max == 1 {
            format!("{} accetta un solo adapter per profilo", manifest.name)
        } else {
            format!("{} accetta al massimo {} adapter per profilo", manifest.name, support.max)
        }));
    }
    for a in adapters {
        if a.path.trim().is_empty() {
            return Err(Error::Invalid("adapter senza percorso".into()));
        }
        if let Some(scale) = a.scale {
            if !scale.is_finite() || scale <= 0.0 {
                return Err(Error::Invalid(format!("la scala dell'adapter {} deve essere un numero maggiore di 0", a.path)));
            }
            if support.scaled_args.is_empty() {
                return Err(Error::Invalid(format!("{} non permette di cambiare la scala di un adapter", manifest.name)));
            }
        }
        check_files(&resolve(&a.path), &support.format).map_err(Error::Invalid)?;
    }
    Ok(())
}

fn name_of(path: &Path) -> String {
    path.file_stem().or_else(|| path.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The launch arguments that load `adapters`; none for a runtime without support.
pub fn launch_args(support: Option<&AdapterSupport>, adapters: &[AdapterRef]) -> Vec<String> {
    let Some(support) = support else { return vec![] };
    if adapters.is_empty() {
        return vec![];
    }
    let mut out = support.base_args.clone();
    for a in adapters {
        let path = resolve(&a.path);
        let template = match a.scale {
            Some(_) if !support.scaled_args.is_empty() => &support.scaled_args,
            _ => &support.args,
        };
        let scale = a.scale.map(|s| s.to_string()).unwrap_or_default();
        out.extend(
            template
                .iter()
                .map(|t| t.replace("{path}", &path.display().to_string()).replace("{name}", &name_of(&path)).replace("{scale}", &scale)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::builtin;

    fn manifest(id: &str) -> RuntimeManifest {
        builtin().into_iter().find(|m| m.id == id).unwrap()
    }

    fn adapter(path: &Path, scale: Option<f32>) -> AdapterRef {
        AdapterRef { path: path.display().to_string(), scale }
    }

    fn mlx_dir(dir: &Path) -> PathBuf {
        let p = dir.join("mlx-adapter");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("adapters.safetensors"), b"x").unwrap();
        std::fs::write(p.join("adapter_config.json"), b"{}").unwrap();
        p
    }

    #[test]
    fn llama_server_gets_lora_and_lora_scaled() {
        let m = manifest("llama-server");
        let args =
            launch_args(m.adapters.as_ref(), &[adapter(Path::new("/a/one.gguf"), None), adapter(Path::new("/a/two.gguf"), Some(0.5))]);
        assert_eq!(args, ["--lora", "/a/one.gguf", "--lora-scaled", "/a/two.gguf", "0.5"]);
        assert!(launch_args(m.adapters.as_ref(), &[]).is_empty());
    }

    #[test]
    fn mlx_lm_gets_adapter_path() {
        let m = manifest("mlx-lm");
        assert_eq!(launch_args(m.adapters.as_ref(), &[adapter(Path::new("/a/mlx-adapter"), None)]), ["--adapter-path", "/a/mlx-adapter"]);
    }

    #[test]
    fn ollama_and_runtimes_without_support_refuse_with_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.gguf");
        std::fs::write(&file, b"x").unwrap();
        let err = validate(&manifest("ollama"), &[adapter(&file, None)]).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert!(err.to_string().contains("ADAPTER") && err.to_string().contains("Modelfile"), "{err}");
        let err = validate(&manifest("lmstudio"), &[adapter(&file, None)]).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)) && err.to_string().contains("non carica adapter"), "{err}");
        // No adapter: nothing to refuse.
        assert!(validate(&manifest("ollama"), &[]).is_ok());
    }

    #[test]
    fn paths_are_checked_against_the_format_the_runtime_wants() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("lora.gguf");
        std::fs::write(&gguf, b"x").unwrap();
        let mlx = mlx_dir(dir.path());
        let llama = manifest("llama-server");
        let mlx_lm = manifest("mlx-lm");
        assert!(validate(&llama, &[adapter(&gguf, Some(0.8))]).is_ok());
        assert!(validate(&mlx_lm, &[adapter(&mlx, None)]).is_ok());

        let why = |r: Result<()>| r.unwrap_err().to_string();
        assert!(why(validate(&llama, &[adapter(&dir.path().join("none.gguf"), None)])).contains("non esiste"));
        let txt = dir.path().join("lora.bin");
        std::fs::write(&txt, b"x").unwrap();
        assert!(why(validate(&llama, &[adapter(&txt, None)])).contains("non è un file GGUF"));
        // A PEFT/MLX folder for llama.cpp: told how to convert it.
        assert!(why(validate(&llama, &[adapter(&mlx, None)])).contains("convert_lora_to_gguf"));
        // A GGUF file for mlx-lm: a folder is needed.
        assert!(why(validate(&mlx_lm, &[adapter(&gguf, None)])).contains("serve la cartella"));
        std::fs::remove_file(mlx.join("adapter_config.json")).unwrap();
        assert!(why(validate(&mlx_lm, &[adapter(&mlx, None)])).contains("mancano adapter_config.json"));
        // Limits and scales.
        assert!(why(validate(&mlx_lm, &[adapter(&mlx, None), adapter(&mlx, None)])).contains("un solo adapter"));
        assert!(why(validate(&mlx_lm, &[adapter(&mlx, Some(0.5))])).contains("scala"));
        assert!(why(validate(&llama, &[adapter(&gguf, Some(0.0))])).contains("maggiore di 0"));
    }

    #[test]
    fn peft_folders_need_config_and_weights() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_files(dir.path(), "peft").unwrap_err().contains("mancano adapter_config.json e adapter_model.safetensors"));
        std::fs::write(dir.path().join("adapter_config.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("adapter_model.safetensors"), b"x").unwrap();
        assert!(check_files(dir.path(), "peft").is_ok());
    }
}
