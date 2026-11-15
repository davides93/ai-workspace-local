# Pubblicazione sugli store (App Store Connect e Google Play)

Workflow: `.github/workflows/store.yml`. Parte da solo con un tag `v*` (iOS e macOS su TestFlight, Android sul track `alpha` con release `completed`; insieme a `release.yml`, che pubblica DMG e Windows sulla GitHub Release) e a mano da *Actions → Publish to stores → Run workflow* per scegliere piattaforme, track e stato. Con un tag la versione di `tauri.conf.json` deve essere quella del tag (`v0.5.0-beta.5` ↔ `0.5.0-beta.5`), altrimenti il workflow si ferma prima di caricare. Gli Environment `app-store` e `google-play`, se hanno l'approvazione manuale, la chiedono a ogni tag.
Bundle ID / package name: `dev.aiworkspace.local` (da `tauri.conf.json`). Nessun segreto nel repo: tutto in **GitHub Secrets**.

Consiglio: crea due *Environment* (Settings → Environments) `app-store` e `google-play` e metti i segreti lì, con approvazione manuale richiesta.

## Segreti da creare (nomi esatti)

### Apple (iOS, iPadOS, macOS)
| Segreto | Contenuto |
|---|---|
| `APPLE_TEAM_ID` | Team ID (10 caratteri, developer.apple.com → Membership) |
| `APPSTORE_API_KEY_ID` | Key ID della chiave API App Store Connect |
| `APPSTORE_API_ISSUER_ID` | Issuer ID (stessa pagina delle chiavi) |
| `APPSTORE_API_KEY_P8` | Contenuto testuale del file `AuthKey_XXXX.p8` |
| `IOS_DISTRIBUTION_CERT_P12_BASE64` | Certificato *Apple Distribution* esportato .p12, in base64 |
| `IOS_DISTRIBUTION_CERT_PASSWORD` | Password del .p12 |
| `IOS_APPSTORE_PROFILE_BASE64` | Provisioning profile *App Store* per iOS (`.mobileprovision`), base64 |
| `MAC_APP_CERT_P12_BASE64` | Certificato *Apple Distribution* (o *3rd Party Mac Developer Application*) .p12, base64 |
| `MAC_INSTALLER_CERT_P12_BASE64` | Certificato *Mac Installer Distribution* (o *3rd Party Mac Developer Installer*) .p12, base64 |
| `MAC_CERT_PASSWORD` | Password usata per esportare i due .p12 Mac (stessa per entrambi) |
| `MAC_APPSTORE_PROFILE_BASE64` | Provisioning profile *Mac App Store* (`.provisionprofile`), base64 |
| `MAC_APP_SIGNING_IDENTITY` | Nome esatto dell'identità app (es. `Apple Distribution: Nome (TEAMID)`) |
| `MAC_INSTALLER_SIGNING_IDENTITY` | Nome esatto dell'identità installer (es. `3rd Party Mac Developer Installer: Nome (TEAMID)`) |

Base64: `base64 -i file.p12 | pbcopy` (macOS).

### Android
| Segreto | Contenuto |
|---|---|
| `ANDROID_UPLOAD_KEYSTORE_BASE64` | Keystore di upload `.jks`, base64 |
| `ANDROID_KEYSTORE_PASSWORD` | Password del keystore |
| `ANDROID_KEY_ALIAS` | Alias della chiave |
| `ANDROID_KEY_PASSWORD` | Password della chiave |
| `PLAY_SERVICE_ACCOUNT_JSON` | JSON del service account Google Cloud (testo intero) |

## Apple: cosa fare a mano (una volta)
1. **App ID**: nelle capability abilita solo quelle che servono. App Sandbox **non** è nell'App ID.
2. **Certificati** (developer.apple.com → Certificates): *Apple Distribution* (iOS e Mac app) e *Mac Installer Distribution*. Creali con una CSR da Keychain Access, scarica, installa, esporta ciascuno come .p12.
3. **Provisioning profile**: uno *iOS App Store* e uno *Mac App Store Connect* per `dev.aiworkspace.local`; scaricali.
4. **Chiave API**: App Store Connect → Utenti e accessi → Integrazioni → Chiavi → ruolo *App Manager*. Scarica il .p8 (una sola volta).
5. Il record app in App Store Connect esiste già (stesso Bundle ID, iOS + macOS). Servono poi: privacy policy URL, categoria, schermate, informazioni di export crittografia.
6. Il numero di build (`bundleVersion` in `tauri.conf.json`, oggi 5013) deve **crescere a ogni upload**; la versione breve viene ricavata dal tag togliendo il suffisso (`0.5.0-beta.2` → `0.5.0`), perché Apple accetta solo `X.Y.Z`.

### App Sandbox (macOS)
Si imposta nel file entitlements del target macOS, non nell'App ID: `apps/desktop/src-tauri/Entitlements.mas.plist` (già nel repo), caricato da `tauri.mas.conf.json`. Chiavi: `com.apple.security.app-sandbox`, `network.client`, `network.server` (daemon/llama-server locali), `files.user-selected.read-write` e `files.bookmarks.app-scope` (le cartelle consentite dall'utente, vedi sotto), `device.audio-input` (dettatura), più `application-identifier` e `team-identifier` (riempiti dalla CI).

### Due versioni per macOS
| | DMG (fuori store, `release.yml`) | Mac App Store (`store.yml`) |
|---|---|---|
| Sandbox | no | sì (`--features mas`) |
| Cartella di lavoro | scelta in Impostazioni → `workspace.root` | quella dell'app (container), non modificabile; in più le cartelle che l'utente consente in Impostazioni → *Cartelle consentite* (permesso che resta dopo il riavvio) |
| Runtime esterni (llama-server, Ollama, vLLM, MLX…) | installabili e avviabili da AIW | non installabili né avviabili; un server già attivo (es. Ollama, LM Studio) viene usato |
| Motore incorporato (`llama-embedded`, Metal), Apple Foundation Models | sì | sì |
| `computer-use` (schermo e input sintetico) | sì | no (la build fallisce se `mas` e `computer-use` sono insieme) |
| Browser dell'agente | Chrome/Edge/Chromium con profilo dedicato | WebKit interno (macOS 14+), separato dalla UI, HTTPS tramite proxy di policy; nessun avvio di Chrome |
| Addestramento (trainer Python) | sì | no |

Apple Foundation Models nella versione sandbox: l'App Sandbox non lascia eseguire un programma che l'app ha scritto da sé (l'helper `aiw-apple-fm` estratto in `app_data_dir/bin` non parte; provato con un'app di prova in sandbox). Per questo `store.yml` copia l'helper compilato da `build.rs` in `Contents/MacOS/aiw-apple-fm`, lo firma con `Entitlements.mas-helper.plist` (`app-sandbox` + `inherit`, richiesti da App Store Connect per i programmi lanciati dall'app) e firma di nuovo l'app; a runtime l'app preferisce l'helper accanto al proprio eseguibile a quello incorporato. Con un'app di prova in sandbox, helper nel bundle con `inherit`: disponibilità, chiamata di strumento e risultato funzionano; la firma con i certificati veri e la validazione di App Store Connect si vedono solo al primo upload.

La UI mostra già "non supportato" con il motivo per ciò che la sandbox impedisce (stato `unsupported` dei runtime) e, in Dispositivi, la riga *Versione: Mac App Store (sandbox)*. Il flag `mas` si legge in `apps/desktop/src-tauri/src/lib.rs` e arriva al motore come `EngineOptions::sandboxed`.

Da verificare su un Mac con la build TestFlight prima di inviare in revisione: avvio, download di un modello dalla gallery, chat con il motore incorporato, accesso a un server Ollama/LM Studio locale, navigazione con l'agente e le **cartelle consentite** (aggiungere Download dal pannello di macOS, chiedere a un agente, anche da Telegram, di elencarla, chiudere e riaprire l'app e ripetere senza riaprire il pannello, togliere la cartella e vedere il rifiuto). Gli strumenti che lanciano programmi esterni (shell) restano soggetti alla sandbox.

### Cartelle consentite (security-scoped bookmark)
Nella build Mac App Store la cartella di lavoro resta quella dell'app, ma l'utente può dare agli agenti altre cartelle del Mac (per esempio *Download* o *Documenti*) da **Impostazioni → Cartelle consentite**. Come funziona:
- **Solo l'utente le aggiunge.** «Aggiungi cartelle…» apre il pannello di apertura di macOS (`NSOpenPanel`, `apps/desktop/plugins/tauri-plugin-aiw-native/macos-folders/Folders.swift`); il comando Tauri `aiw_folders_add` della shell è l'unico punto che cambia l'elenco (con `aiw_folders_remove`). Non esiste una rotta dell'API che aggiunge una cartella (`GET /api/folders` è di sola lettura e solo per il proprietario: ogni altro metodo dà 405), né un'impostazione, uno strumento o un parametro del modello: un agente non può allargare da solo il proprio accesso. Un browser o un altro dispositivo vede l'elenco ma non lo cambia.
- **Il permesso resta dopo il riavvio.** La scelta diventa un *security-scoped bookmark* (entitlement `com.apple.security.files.bookmarks.app-scope`, accanto a `files.user-selected.read-write`). Nel database dell'app (`folder_grants`) c'è solo il permesso, mai un percorso: dov'è la cartella lo dice macOS a ogni avvio. All'avvio la shell riapre i permessi **prima** che partano i canali (un messaggio Telegram arrivato subito trova già le cartelle), rinnova un permesso scaduto (cartella spostata o rinominata) e mostra «Non disponibile» per una cartella che non si riapre (cancellata, ad esempio), senza darla agli agenti.
- **Cosa possono fare gli agenti.** Gli strumenti file (`read_file`, `write_file`, `edit_file`, `list_dir`, `glob`, `grep`, `move_file`, `copy_file`) lavorano nelle cartelle consentite come nel workspace: lettura e scrittura, mai di più (`ToolContext::write_roots`, separato da `read_roots`, che restano in sola lettura per skill e allegati). Valgono ancora la modalità di approvazione, le regole di policy e la sandbox dell'agente (con `read-only` nessuna scrittura, nemmeno lì). **`delete_file` rifiuta fuori dal workspace**: il ripristino di un run (`rewind`) rimette a posto solo il workspace, i file fuori sono solo elencati; un file sovrascritto lì tiene l'originale nel checkpoint (`_esterni/…`) ma non viene rimesso da un ripristino.
- **Cosa resta nascosto.** Anche dentro una cartella consentita restano rifiutate la cartella dei dati dell'app e i luoghi di `sandbox.deny_read` (`~/.ssh`, portachiavi…), calcolati anche sulla **vera** cartella personale (nella sandbox `~` è il container). Non si può consentire: tutto il disco, la cartella personale intera o una che la contiene, `~/Library`, le cartelle di sistema, il container dell'app e le cartelle che contengono i dati dell'app.
- **Il modello sa dove sono.** Nel blocco *Ambiente* del prompt ci sono i percorsi veri delle cartelle consentite (`~` è il container: `~/Downloads` non porta alla cartella vera; un percorso scritto così viene comunque risolto se finisce in una cartella consentita). Quando uno strumento rifiuta un percorso «fuori dal workspace», il messaggio dice dove l'utente può consentirlo (lo ripete anche Telegram) e la scheda dello strumento nella chat rimanda a Impostazioni.
- **Limiti noti, non coperti.** Il tool `shell` e i server MCP stdio **non** ricevono le cartelle consentite (un processo figlio non eredita i permessi del bookmark: non provato); le scritture dei comandi restano limitate al workspace. Un agente con un modello remoto può mandare ai provider ciò che legge nelle cartelle consentite (vale per ogni file letto, ora su una superficie più grande: va detto nell'informativa sulla privacy e nell'etichetta *App Privacy*).

**Provato in locale (macOS, app di prova firmata ad hoc con App Sandbox e gli stessi entitlement):** `bash apps/desktop/plugins/tauri-plugin-aiw-native/macos-folders/selftest.sh` crea il bookmark di una cartella del container, lo riapre tramite la stessa ABI C che usa la shell, controlla che la sandbox rifiuti davvero la vera `~/Downloads` prima del permesso, che il permesso si rilasci, che una cartella **rinominata** si ritrovi con un permesso rinnovato, che una cartella cancellata o un testo che non è un bookmark diano un errore leggibile e che **un secondo avvio** riapra il permesso salvato (27 controlli). Il contenitore `~/Downloads` dell'app è davvero un collegamento alla vera cartella (verificato nello stesso test: è per questo che il percorso scritto dal modello viene risolto dove atterra). Il binario `aiw-desktop` con `--features mas` si collega (`cargo build -p aiw-desktop --features mas`) e, firmato ad hoc in App Sandbox con un identificativo di prova, parte e resta attivo con il ponte registrato e i permessi riaperti all'avvio (senza interazione: nessun clic sul pannello). La parte Rust (confinamento dei percorsi, elenco, persistenza, rinnovo, rifiuti, nessuna via per allargare i permessi) ha test automatici con un ponte finto.
**Non provato:** il pannello `NSOpenPanel` con un clic su una cartella fuori dal container (`bash …/selftest.sh --pick`, a mano: sceglie una cartella, la elenca dall'interno della sandbox, poi un secondo avvio la riapre senza pannello), il comportamento su una build vera firmata con i certificati e il provisioning profile dello store, la prova da Telegram con la build TestFlight, altre versioni di macOS oltre a quella di sviluppo, e il fatto che `bookmarks.app-scope` sia davvero richiesto (la documentazione e i forum di Apple sono discordi; è aggiunto perché non costa nulla in revisione).

**Per la revisione Apple.** Nessuna eccezione temporanea di sandbox e nessuna chiave `NS…UsageDescription` (il pannello di apertura non ne ha bisogno: sono per l'accesso fuori sandbox). Il rischio è l'uso dichiarato dell'accesso ai file: nelle note per la revisione scrivere che l'app dà agli agenti solo le cartelle scelte dall'utente col pannello di apertura di macOS, mai cartelle scelte dall'app, e che l'utente può toglierle da Impostazioni in qualsiasi momento; verificare che la descrizione della privacy dica che il contenuto dei file letti dagli agenti può essere inviato al provider del modello se l'utente ne configura uno remoto. Da controllare al primo upload che App Store Connect accetti `files.bookmarks.app-scope` con il provisioning profile (non provato).

Il flag `mas` abilita `tauri-plugin-aiw-native/macos-browser` e `macos-folders`: una WKWebView AppKit collegata staticamente all'app, senza IPC Tauri sulle pagine remote. Usa il proxy Rust e gli stessi strumenti del browser mobile. La finestra può essere chiusa e riaperta; il kill switch interrompe il caricamento e svuota la pagina. Su macOS precedenti al 14 viene mostrato un errore esplicito, senza tentare Chrome. Verifica locale in un'app di prova firmata ad hoc con App Sandbox: `bash apps/desktop/plugins/tauri-plugin-aiw-native/macos-browser/selftest.sh`; aggiungere `--online` per caricare anche example.com attraverso un tunnel HTTPS. Non modifica l'app installata. Le due parti Swift (browser e cartelle) sono compilate in una sola libreria statica (`libaiw_macos_native`) e condividono `native_call.rs`.

## Cosa esce dove
- **GitHub Release** (`release.yml`): solo il `.dmg` macOS e l'installer Windows `.exe`.
- **Mac App Store, iOS e Android** (`store.yml`): solo da qui, mai come file della release.

## Android: guida passo-passo
1. **Account Play Console** (play.google.com/console): 25 $ una tantum, verifica identità. Per account personali nuovi, per andare in produzione servono 12 tester per 14 giorni di test chiuso.
2. **Crea l'app**: nome AIW, lingua italiana, app gratuita, accetta le dichiarazioni. Il package name sarà `dev.aiworkspace.local` (fissato al primo upload, non modificabile).
3. **Play App Signing**: lascialo attivo (default). Google custodisce la chiave di firma; tu firmi con una *upload key*.
4. **Upload keystore** (sul tuo computer, conserva il file in un posto sicuro):
   `keytool -genkeypair -v -storetype JKS -keystore upload.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000`
   Poi `base64 -i upload.jks | pbcopy` per il segreto `ANDROID_UPLOAD_KEYSTORE_BASE64`.
5. **Service account per la CI**: Google Cloud Console → crea progetto → abilita *Google Play Android Developer API* → IAM → Service accounts → crea, poi *Keys → Add key → JSON*. In Play Console → Utenti e autorizzazioni → Invita utente con l'email del service account e i permessi *Rilascia nei track di test* (e produzione quando serve) per l'app AIW.
6. **Primo upload manuale**: crea una release nel track *Test interno*, carica un AAB (anche quello prodotto dal workflow: scaricalo con `play_status=draft`, oppure lancia la CI e completa la bozza a mano), aggiungi i tester (lista email). Google richiede il primo upload dalla console prima che l'API accetti release complete; finché l'app non è pubblicata, la CI deve usare `status: draft`.
7. Compila le sezioni obbligatorie: scheda store, classificazione contenuti, sicurezza dei dati, privacy policy, target di pubblico, dichiarazione sugli annunci.
8. Da lì: `Run workflow` con `android` attivo → release in bozza nel track scelto → la confermi in console.

## Come si lancia
Actions → *Publish to stores* → scegli piattaforme e track. iOS e macOS arrivano su TestFlight (poi "Invia per revisione" da App Store Connect); Android arriva nel track scelto (predefinito: `alpha`, il test chiuso, con stato `completed`: la release parte subito ai tester; usa `draft` per confermarla a mano in console).

Nota: il workflow non è stato eseguito (richiede i segreti); i primi lanci vanno fatti una piattaforma alla volta.
