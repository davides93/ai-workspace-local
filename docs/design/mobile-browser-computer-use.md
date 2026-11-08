# Browser e computer use su mobile: piano

Stato: proposta, nessun codice scritto. Oggi `browser_*` e `computer_*` sono solo desktop (`Tool::available()` risponde Err su iOS e Android).

## Cosa è possibile

| | iOS / iPadOS | Android |
|---|---|---|
| Browser controllato | Sì, con una WKWebView in-app | Sì, con una WebView in-app |
| Computer use (altre app) | **No**: il sistema non permette di comandare né catturare altre app | Sì, con un AccessibilityService (sperimentale) |
| Alternative | Comandi Rapidi (`shortcut_run`), `open_url`, App Intents | `computer_snapshot` dall'albero di accessibilità |

## 1. Browser: un'astrazione, due backend
- Trait `BrowserBackend` in `aiw-tools::browser`: `available`, `navigate(url) -> url finale`, `eval(js)`, `screenshot()`, `close`.
- `BrowserTool` usa `Arc<dyn BrowserBackend>`. Restano comuni a tutti: `SNAPSHOT_JS`, click e type, `vet_with` prima della navigazione, controllo dell'URL finale.
- `BrowserManager` diventa `CdpBackend` (desktop, invariato). Su mobile l'app registra un `WebViewBackend` con `Engine::set_browser_backend`, come già fatto per `NativeGenerate`; le chiamate al plugin vanno in `spawn_blocking`.
- La WebView è un pannello **visibile** (bottom sheet su telefono, vista affiancata su iPad): iOS sospende le WebView nascoste e l'utente deve poter fermare l'agente o fare il login. È una WebView nativa semplice, **mai tramite Tauri**, per non esporre l'IPC dell'app alle pagine. Dati separati dal resto dell'app.
- **Policy di rete**: i delegate (`decidePolicyFor`, `shouldInterceptRequest`) non controllano il DNS. Si usa un proxy HTTP CONNECT in Rust su 127.0.0.1 che applica `vet_url_with` e il resolver verificato, con registro dei blocchi. La WebView passa dal proxy con `ProxyController` su Android e `proxyConfigurations` su iOS 17+. Sotto iOS 17 il tool è non disponibile. WebRTC va disattivato.
- Snapshot, click e type: lo stesso JS (`callAsyncJavaScript` in un content world isolato su iOS, `evaluateJavascript` su Android). Screenshot: `takeSnapshot` su iOS, `PixelCopy` su Android, JPEG a circa 1280 px.

## 2. Computer use
- **Android**: trait `ComputerBackend` (enigo/xcap su desktop, plugin su Android). `dispatchGesture` per tap e scroll, `ACTION_SET_TEXT` per scrivere, `performGlobalAction` per Back/Home, `takeScreenshot` (API 30+). Nuovo tool `computer_snapshot` basato sull'albero di accessibilità, più affidabile delle coordinate con i modelli piccoli.
- Attivazione manuale nelle Impostazioni di sistema, disattivato di default. Android 13+ blocca le APK fuori dallo store finché l'utente non sblocca le impostazioni con restrizioni.
- Foreground service con notifica e Stop, overlay "agente attivo", approvazione per azione o per sessione a tempo, elenco di app escluse (Impostazioni, banche, password manager, l'app stessa). Le finestre `FLAG_SECURE` danno screenshot neri.
- **Google Play** probabilmente rifiuta l'uso non accessibilistico dell'AccessibilityService: distribuzione solo come APK o F-Droid.
- **iOS**: i tool `computer_*` restano non disponibili con un messaggio chiaro. Si offrono `shortcut_run` (l'utente scrive i nomi dei Comandi Rapidi, iOS non li elenca), `open_url` e App Intents per esporre l'agente a Siri.

## 3. Fasi
| Fase | Contenuto | Verifica in CI |
|---|---|---|
| P0 | Trait `BrowserBackend` + `CdpBackend` | Test esistenti + `FakeBackend` (policy, redirect, ref) |
| P1 | Proxy di policy in Rust | Test su Linux: CONNECT ammesso e bloccato, rebinding con resolver finto |
| P2 | Backend e pannello Android | `cargo check` per Android, build Gradle, test su emulatore |
| P3 | Backend e pannello iOS | Simulatore iOS su runner macOS |
| P4 | Computer use Android | Emulatore con servizio abilitato via `adb` |
| P5 | iOS: Comandi Rapidi e App Intents | Solo build, test manuale |

## Rischi
Sotto iOS 17 niente proxy; le app in background vengono uccise; click sintetici (`isTrusted=false`) come oggi su desktop; loopback condiviso su Android (serve un token); rifiuto di Google Play.

## Parità funzionale oggi
| Funzione | Windows | macOS | Linux | Android | iOS / iPadOS |
|---|---|---|---|---|---|
| Agenti, workflow, MCP (HTTP), file, web fetch | ✅ | ✅ | ✅ | ✅ | ✅ |
| Shell e MCP stdio | ✅ | ✅ | ✅ | ✅ | ❌ regola di iOS |
| Browser controllato | ✅ serve Chrome | ✅ | ✅ | ❌ | ❌ |
| Computer use | ✅ | ✅ | ❌ escluso in CI | ❌ | ❌ |
| Modello di sistema | ❌ Phi Silica non collegato | 🟡 Apple FM da provare | — | 🟡 Gemini Nano da provare | 🟡 Apple FM da provare |
| GGUF locali, galleria | ✅ | ✅ Metal | ✅ | ✅ | ✅ Metal |
