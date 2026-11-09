# Pubblicazione sugli store (App Store Connect e Google Play)

Workflow: `.github/workflows/store.yml` (manuale, *Actions → Publish to stores → Run workflow*).
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
6. Il numero di build (`bundleVersion` in `tauri.conf.json`, oggi 5004) deve **crescere a ogni upload**; la versione breve viene ricavata dal tag togliendo il suffisso (`0.5.0-beta.2` → `0.5.0`), perché Apple accetta solo `X.Y.Z`.

### App Sandbox (macOS)
Si imposta nel file entitlements del target macOS, non nell'App ID: `apps/desktop/src-tauri/Entitlements.mas.plist` (già nel repo), caricato da `tauri.mas.conf.json`. Chiavi: `com.apple.security.app-sandbox`, `network.client`, `network.server` (daemon/llama-server locali), `files.user-selected.read-write`, `device.audio-input` (dettatura), più `application-identifier` e `team-identifier` (riempiti dalla CI).

### Due versioni per macOS
| | DMG (fuori store, `release.yml`) | Mac App Store (`store.yml`) |
|---|---|---|
| Sandbox | no | sì (`--features mas`) |
| Cartella di lavoro | scelta in Impostazioni → `workspace.root` | quella dell'app (container), non modificabile |
| Runtime esterni (llama-server, Ollama, vLLM, MLX…) | installabili e avviabili da AIW | non installabili né avviabili; un server già attivo (es. Ollama, LM Studio) viene usato |
| Motore incorporato (`llama-embedded`, Metal), Apple Foundation Models | sì | sì |
| `computer-use` (schermo e input sintetico) | sì | no (la build fallisce se `mas` e `computer-use` sono insieme) |
| Addestramento (trainer Python) | sì | no |

La UI mostra già "non supportato" con il motivo per ciò che la sandbox impedisce (stato `unsupported` dei runtime) e, in Dispositivi, la riga *Versione: Mac App Store (sandbox)*. Il flag `mas` si legge in `apps/desktop/src-tauri/src/lib.rs` e arriva al motore come `EngineOptions::sandboxed`.

Da verificare su un Mac con la build TestFlight prima di inviare in revisione: avvio, download di un modello dalla gallery, chat con il motore incorporato, accesso a un server Ollama/LM Studio locale. Gli strumenti dell'agente che lanciano programmi (shell, browser CDP) restano soggetti alla sandbox.

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
Actions → *Publish to stores* → scegli piattaforme e track. iOS e macOS arrivano su TestFlight (poi "Invia per revisione" da App Store Connect); Android arriva nel track scelto.

Nota: il workflow non è stato eseguito (richiede i segreti); i primi lanci vanno fatti una piattaforma alla volta.
