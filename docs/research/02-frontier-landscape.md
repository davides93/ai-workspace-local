# 02 · Panorama di frontiera (novembre 2026)

Questo documento fissa lo stato dell'arte usato per progettare il catalogo dei runtime e dei layer
agentici. Ogni voce diventa un **manifest** nel registry (`crates/aiw-runtimes/manifests/`).
Le voci marcate con † sono recenti o in preview: vanno riverificate a ogni release.

## 1. Inferenza locale: tre famiglie di integrazione

| Famiglia | Come la integra AIW | Esempi |
|---|---|---|
| **Embedded** (in-process, FFI) | Libreria linkata nel core o nel plugin nativo; nessun processo esterno | llama.cpp, ONNX Runtime GenAI, MLX, LiteRT-LM, ExecuTorch, MLC-LLM, mistral.rs |
| **Sidecar gestito** | AIW avvia, monitora (health check) e ferma il processo; parla HTTP OpenAI-compatibile | llama-server, Ollama, LM Studio, vLLM, SGLang, TensorRT-LLM, mlx-lm, Foundry Local, Lemonade, OpenVINO MS, LocalAI, KoboldCpp, llamafile, TabbyAPI, Nexa, exo |
| **Modello di sistema** | API del sistema operativo, modello condiviso già sul dispositivo | Apple Foundation Models, Gemini Nano (ML Kit GenAI / AICore), Windows AI APIs (Phi Silica), Chrome Prompt API |
| **Endpoint remoto in LAN** | Un altro dispositivo AIW o un server compatibile nella rete locale | Desktop con GPU che serve il telefono; worker RPC llama.cpp; cluster exo |

## 2. Matrice runtime × piattaforma

Legenda: ● supporto nativo · ◐ parziale, preview o via layer di compatibilità · — non disponibile

| Runtime | Win | macOS | Linux | Android | iOS | Acceleratori | Formati |
|---|---|---|---|---|---|---|---|
| llama.cpp (embedded / `llama-server`) | ● | ● | ● | ● | ● | CPU (AVX2/AVX-512/AMX/NEON/SVE), CUDA, HIP/ROCm, Vulkan, Metal, SYCL, OpenCL (Adreno), CANN, MUSA, Hexagon†, RPC | GGUF |
| Ollama | ● | ● | ● | — | — | CUDA, ROCm, Metal, Vulkan† | GGUF / Modelfile |
| LM Studio (`lms`) | ● | ● | ● | — | — | llama.cpp + motore MLX | GGUF, MLX |
| vLLM | ◐ (WSL) | ◐ † | ● | — | — | CUDA, ROCm, Intel XPU, TPU, CPU | safetensors, AWQ/GPTQ/FP8, GGUF◐ |
| SGLang | ◐ (WSL) | — | ● | — | — | CUDA, ROCm | safetensors |
| TensorRT-LLM (`trtllm-serve`) | ◐ | — | ● | — | — | NVIDIA | engine TRT / HF |
| MLX / mlx-lm / mlx-vlm / MLX Swift | — | ● | ◐ (CUDA†) | — | ● | Apple GPU (Metal), memoria unificata | MLX safetensors |
| Apple Foundation Models | — | ● | — | — | ● | ANE/GPU (modello di sistema ~3B) + Private Cloud Compute | di sistema (+ adapter) |
| Apple Core AI † / Core ML | — | ● | — | — | ● | ANE/GPU/CPU | `.mlpackage` / Core AI |
| ONNX Runtime GenAI | ● | ● | ● | ● | ● | DirectML, CUDA, TensorRT-RTX, QNN, OpenVINO, VitisAI, CoreML, XNNPACK | ONNX |
| Windows ML (ORT di sistema + EP scaricati) | ● | — | — | — | — | NPU Qualcomm/Intel/AMD, GPU (Win11 24H2+) | ONNX |
| Foundry Local | ● | ● | — | — | — | CPU/GPU/NPU con rilevamento automatico | ONNX (catalogo) |
| Windows AI APIs (Phi Silica) | ● (Copilot+ PC) | — | — | — | — | NPU | di sistema |
| AMD Lemonade Server | ● | — | ● | — | — | Ryzen AI NPU+iGPU (OGA), Vulkan, ROCm | ONNX/OGA, GGUF |
| OpenVINO GenAI / Model Server | ● | ◐ | ● | — | — | CPU/GPU/NPU Intel | OpenVINO IR |
| LiteRT-LM | ● (CLI/C++) | ● | ● | ● (Kotlin, stabile) | ◐ (Swift, preview) | CPU, GPU (OpenCL/Metal), NPU (Android) | `.litertlm` (Gemma 3n/4…) |
| ML Kit GenAI Prompt API (Gemini Nano / AICore) | — | — | — | ● (device supportati) | — | NPU/TPU del SoC | di sistema |
| ExecuTorch 1.x | ● | ● | ● | ● | ● | XNNPACK, CoreML, MPS, QNN, Vulkan, MediaTek, OpenVINO | `.pte` |
| MLC-LLM | ● | ● | ● | ● | ● | Vulkan, Metal, CUDA, OpenCL, WebGPU | MLC |
| mistral.rs / candle | ● | ● | ● | ◐ | ◐ | CUDA, Metal, CPU | GGUF, safetensors |
| Nexa SDK | ● | ● | ● | ● | ● | NPU Qualcomm/Apple/Intel/AMD | GGUF, MLX, NEXA |
| exo (cluster tra dispositivi) | ◐ | ● | ● | — | ◐ | multi-device (MLX/tinygrad) | MLX/safetensors |
| WebLLM / transformers.js / Chrome Prompt API | browser | browser | browser | browser | browser | WebGPU / WebNN / Gemini Nano in Chrome | MLC / ONNX |

Modalità aggiuntive, con lo stesso modello a manifest: **STT** (whisper.cpp, sherpa-onnx,
SpeechAnalyzer di Apple, SpeechRecognizer di Android), **TTS** (Kokoro, Piper, voci di sistema),
**embedding e reranking** (llama.cpp `--embedding`, EmbeddingGemma, bge/gte in ONNX),
**immagini** (stable-diffusion.cpp, Core ML SD). Sono in roadmap.

## 3. Segnali chiave del 2026

- **Apple (WWDC26, iOS/macOS 27)**: Foundation Models si estende (composizione di modelli,
  framework Evaluations, CLI `fm`, SDK Python) e arriva su watchOS 27 tramite Private Cloud
  Compute. Arriva **Core AI** come successore di Core ML per i modelli personalizzati; MLX Swift
  resta il terzo pilastro. ([Apple WWDC26](https://developer.apple.com/wwdc26/guides/ios/), [InfoQ](https://infoq.com/news/2026/06/apple-core-ai-wwdc))
- **Google**: LiteRT-LM aggiunge API Swift (preview) e JavaScript, integrazione con il framework
  Foundation di Apple (v0.15) e Gemma 4 con drafter MTP, *thinking* e function calling.
  ML Kit GenAI Prompt API su Gemini Nano (nano-v2/v3) permette l'inferenza **solo in foreground**,
  con quota per app (errori `BACKGROUND_USE_BLOCKED` e `BUSY`).
  ([Google Developers Blog](https://developers.googleblog.com/en/blazing-fast-on-device-genai-with-litert-lm/), [ML Kit GenAI](https://developers.google.com/ml-kit/genai))
- **Microsoft**: Windows ML fornisce un ONNX Runtime di sistema con *execution provider* scaricati
  dinamicamente per NPU e GPU (Win11 24H2+). Il percorso consigliato: Windows AI APIs per i modelli
  integrati, Foundry Local per gli LLM locali, Windows ML per pipeline ONNX personalizzate.
  ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview), [AMD](https://amd.com/en/developer/resources/technical-articles/2026/ai-model-deployment-using-windows-ml-on-amd-npu.html))
- **Server GPU**: vLLM 0.16+ copre NVIDIA, ROCm, Intel XPU e TPU. llama.cpp resta lo standard
  trasversale (tutti i sistemi operativi e tutti i backend GPU).

## 4. Modelli per computer-use / GUI agent (open-weight)

| Modello | Note |
|---|---|
| Holo3-35B-A3B, Holo2-8B/30B-A3B (H Company) | Tra i migliori open-weight su OSWorld; MoE → efficienti in locale |
| Qwen3.5 (27B, 35B-A3B) e Qwen3-VL | VLM generalisti con grounding GUI |
| UI-TARS / UI-TARS-1.5-7B (ByteDance) | Agente GUI end-to-end (verificare la licenza delle versioni 2.x) |
| UI-Mate-27B (Tencent, Apache-2.0)†, EvoCUA-32B (Meituan) | Nuovi, da validare |
| Fara-7B (Microsoft), OpenCUA | Agenti CUA compatti |

Implicazione di design: il tool `computer` deve funzionare in **due modalità**. La prima è
**grounded vision**: screenshot e coordinate, per i VLM che le supportano. La seconda è
**accessibility-first**: albero AX, UIA o AT-SPI serializzato in testo, per i modelli solo testo
e per i dispositivi piccoli. Va scelta automaticamente in base alle capacità del modello attivo.

## 5. Protocolli

| Protocollo | Ruolo in AIW |
|---|---|
| **MCP** (stdio, streamable HTTP, elicitation, OAuth) | Client verso i tool esterni; server per esporre i tool del workspace ad altri agenti |
| **AG-UI 1.0** | Formato degli eventi del run verso la UI (e verso client CopilotKit di terze parti) |
| **A2UI v0.9** / **MCP Apps** | Generative UI dichiarativa o aperta (roadmap: renderer A2UI nella chat) |
| **A2A** | Delega tra agenti su dispositivi diversi (roadmap device mesh) |
| **ACP** (Agent Client Protocol) | Integrazione con gli editor (roadmap) |
| **OpenAI Chat Completions / Responses**, **Anthropic Messages**, **Ollama API** | Gateway: qualunque app (anche Claude Code, Codex o OpenCode) può usare i modelli locali del workspace |

## 6. Agenti personali e "agent PC"

OpenClaw (gateway sempre attivo, canali, skill), Perplexity Personal Computer (agente 24/7,
routing tra più modelli, audit trail, kill switch), OpenDots/OpenBot (un computer per agente),
Hermes Agent (Nous, local-first). Tendenza: **un agente con il proprio computer, sempre attivo,
con canali multipli e permessi espliciti**. AIW la porta in locale: lo stesso core gira sul PC
come daemon e sul telefono dentro l'app, e i dispositivi si accoppiano.

Fonti: [Perplexity PC su Windows – SiliconANGLE](https://siliconangle.com/2026/07/28/perplexity-brings-personal-computer-ai-agent-windows/) ·
[Perplexity PC su Mac – TechCrunch](https://techcrunch.com/2026/05/07/perplexitys-personal-computer-is-now-available-everyone-on-mac/) ·
[OpenDots README](https://raw.githubusercontent.com/CopilotKit/OpenDots/HEAD/README.md) ·
[AG-UI & A2UI – CopilotKit](https://copilotkit.ai/ag-ui-and-a2ui) ·
[A2UI v0.9 – Google](https://developers.googleblog.com/a2ui-v0-9-generative-ui/) ·
[OpenClaw – docs community](https://agentic-ai.readthedocs.io/en/latest/AgentPlatforms/openclaw/) ·
[Codex App Server](https://developers.openai.com/codex/app-server) ·
[LiteRT-LM](https://github.com/google-ai-edge/LiteRT-LM) ·
[Local inference 2026 – starmorph](https://blog.starmorph.com/blog/local-llm-inference-tools-guide) ·
[OSWorld leaderboard – BenchLM](https://benchlm.ai/best/computer-use)
