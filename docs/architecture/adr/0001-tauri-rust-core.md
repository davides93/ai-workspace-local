# ADR 0001 · Tauri 2 + core Rust per tutte le piattaforme

- Stato: accettata · 2026-11-01

## Contesto
Servono Windows, macOS, Linux, Android e iOS con inferenza nativa per piattaforma
(Metal/MLX/Foundation Models su Apple, Gemini Nano/LiteRT-LM su Android, ONNX/Windows ML su
Windows, CUDA/Vulkan/ROCm su PC) e un'unica UI ricca (chat in streaming, editor di workflow,
form di impostazioni generati).

## Decisione
- Il core è una libreria Rust (`crates/*`) compilata per tutti i target.
- La shell è Tauri 2 (desktop e mobile) con una UI React condivisa (`apps/ui`).
- Le funzioni che esistono solo come API native (Foundation Models, ML Kit GenAI, LiteRT-LM,
  AccessibilityService) vivono in un plugin Tauri con codice Swift e Kotlin
  (`apps/desktop/plugins/aiw-native`) e si registrano nel core come runtime `system`.
- Lo stesso core gira anche come daemon (`aiw serve`) e come CLI.

## Conseguenze
- (+) Un solo motore agentico e un solo registry; binari piccoli; accesso diretto a C/C++
  (llama.cpp, ORT) tramite FFI.
- (+) Su desktop la WebView di sistema evita di distribuire Chromium; su mobile si usano WKWebView e Android WebView.
- (−) Le differenze tra WebView richiedono di testare la UI su tutti i motori; le build mobile
  richiedono gli SDK nativi (CI dedicata).
