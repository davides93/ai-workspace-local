# Come si fa una release

Questa pagina descrive la procedura **come la implementano i flussi di lavoro** in `.github/workflows/`. Se un flusso cambia,
cambia anche questa pagina. Stato onesto: i lavori arm64 e CUDA descritti sotto sono **scritti ma mai eseguiti su GitHub**
(vedi «Cosa non è stato provato»); la firma del codice è pronta ma attiva solo con i segreti ([FIRMA.md](FIRMA.md)).

| Flusso | Quando parte | Cosa fa |
| --- | --- | --- |
| `release.yml` | tag `v*`, oppure a mano (*Run workflow*) | costruisce DMG macOS universale ed EXE Windows x64 (più, se richiesti, arm64 e CUDA), crea la GitHub Release |
| `store.yml` | tag `v*`, oppure a mano | iOS e macOS (Mac App Store) su TestFlight, Android (AAB) su Google Play: [docs/store/README.md](../store/README.md) |
| `release-check.yml` | pull request che toccano il flusso di release, `scripts/ci/` o i manifest Cargo/Tauri | `cargo check` su Linux arm64 e Windows arm64 (daemon e shell Tauri), così un target nuovo non si prova per la prima volta su un tag |
| `ci.yml`, `audit.yml` | pull request e `main`; audit anche ogni lunedì | prove normali e controllo delle dipendenze |

## 1. Preparare la versione (una pull request «Prepare vX.Y.Z»)

Il modello è il commit `d002a75` («Prepare v0.5.0-beta.8 release (store build 5010)», `git show --stat d002a75`). Si
cambiano questi file, tutti insieme:

| File | Cosa |
| --- | --- |
| `Cargo.toml` | `version` in `[workspace.package]` (la ereditano tutti i crate e `aiw-desktop`) |
| `apps/desktop/plugins/tauri-plugin-aiw-native/Cargo.toml` | `version` (questo crate la dichiara a parte) |
| `Cargo.lock` | si aggiorna da solo con un `cargo check` (o `cargo update --workspace`): cambiano solo le righe dei crate del workspace |
| `apps/desktop/src-tauri/tauri.conf.json` | `version`; `bundle.android.versionCode`, `bundle.macOS.bundleVersion`, `bundle.iOS.bundleVersion` (il numero di build degli store, vedi sotto) |
| `apps/desktop/package.json`, `apps/desktop/package-lock.json` | `version` |
| `apps/ui/package.json`, `apps/ui/package-lock.json` | `version` |
| `CHANGELOG.md` | il contenuto di «Non rilasciato» passa sotto un titolo `## X.Y.Z - pre-release` (o senza «pre-release» per una stabile); «Non rilasciato» resta, vuoto, in cima |
| `docs/release-notes/vX.Y.Z.md` | le note della release: **obbligatorie**, `release.yml` le legge con `body_path` (senza il file la pubblicazione fallisce alla fine, dopo tutte le build). Si parte da [TEMPLATE.md](../release-notes/TEMPLATE.md) |

Numeri di build degli store: `store.yml` ricava la versione breve dal tag (`0.5.0-beta.8` → `0.5.0`, Apple accetta solo
`X.Y.Z`), ma il numero di build (`bundleVersion`, `versionCode`) **deve crescere a ogni upload** e va scelto a mano. Per
ripetere un upload senza cambiare versione si alza solo quello. `bundle.windows.wix.version` (`0.5.0.2`) è la versione
dell'MSI: oggi la CI non costruisce l'MSI (solo NSIS), quindi non entra nella procedura.

Prima del tag: `cargo fmt --all --check`, `cargo clippy --all-targets`, `cargo test`, e in `apps/ui` `npm run typecheck`,
`npm test`, `npm run build` (la stessa cosa fa `ci.yml`). Aggiorna anche `docs/STATUS.md` se qualcosa è cambiato (vedi
[CONTRIBUTING.md](../../CONTRIBUTING.md): «scritto ≠ provato»).

## 2. Il tag

```bash
git switch main && git pull
git tag v0.5.0-beta.9
git push origin v0.5.0-beta.9
```

Il tag fa partire insieme `release.yml` e `store.yml`.

- `store.yml` controlla che il tag sia `v` + la versione di `tauri.conf.json` e si ferma prima di caricare se non coincidono.
- `release.yml` **non** fa questo controllo: se tag e file non coincidono, la GitHub Release esce con un nome sbagliato
  (e l'aggiornamento dall'app, che legge i tag, si confonde). Controlla a mano prima di spingere il tag.
- Un tag con un `-` (`-beta.N`, `-rc.N`) crea una **pre-release**; senza, una release normale (`prerelease: contains(tag, '-')`).
- Gli Environment `app-store` e `google-play`, se hanno l'approvazione manuale, la chiedono a ogni tag.

In alternativa, da *Actions → Release → Run workflow* si può indicare un `tag`: la release costruisce e pubblica sul commit
scelto e crea il tag lei (utile dove non si possono spingere tag). Con `tag` vuoto costruisce soltanto (nessuna release, gli
artefatti restano scaricabili dalla pagina dell'esecuzione).

## 3. Cosa fa `release.yml`

1. **`select`** decide che cosa costruire (`scripts/ci/select-targets.sh`, con `--self-test` per provarlo in locale).
2. **`desktop`** costruisce le app Tauri: per ogni target un `npx tauri build`, con la firma se i segreti ci sono.
3. **`core-tests`** esegue `cargo test --no-fail-fast` su macOS e Windows (solo se è selezionato un target di default).
4. **`daemon-arm64`**, **`cuda`** e **`vulkan`**: i lavori facoltativi, vedi sotto. Non fermano mai la release.
5. **`publish`** (solo con un tag, o con *Run workflow* e un `tag`): scarica gli artefatti `aiw-*`, rinomina, calcola
   `SHA256SUMS.txt`, crea la release **come bozza** con tutti i file e solo dopo la pubblica. Non parte se `desktop` è fallito.

Il repository ha le **release immutabili**: i file si possono allegare solo mentre la release è una bozza, e pubblicarla
blocca per sempre release e tag. Per questo la release nasce come bozza con tutti i file.

### Target e interruttori

Senza input e senza variabile il tag costruisce **solo Windows x64 e macOS universale**, come prima. Gli altri target si
chiedono così:

- a mano: *Run workflow* → campo `targets` (per esempio `linux-x64,vulkan`, oppure `cuda,windows-arm64`);
- per i tag: variabile del repository `AIW_RELEASE_TARGETS` (Settings → Secrets and variables → Actions → Variables), per
  esempio `default,arm64` o `all`. Si toglie per tornare al default. Il campo `targets` di *Run workflow*, se non è vuoto, vince.

| Target | Gruppo | Che cosa costruisce | File nella release |
| --- | --- | --- | --- |
| `windows-x64` | `default` | app, NSIS | `AI_Workspace_Local_<v>_x64-setup.exe` |
| `macos` | `default` | app, DMG universale | `AI_Workspace_Local_<v>_universal.dmg` |
| `linux-x64` | `linux` | app, `deb`, `AppImage` e `rpm` su `ubuntu-22.04` | `AI_Workspace_Local_<v>_amd64.deb`, `…_amd64.AppImage`, `AI_Workspace_Local-<ver>-<rel>.x86_64.rpm` |
| `linux-arm64` | `linux`, `arm64` | app, `deb`, `AppImage` e `rpm` su `ubuntu-22.04-arm` | `AI_Workspace_Local_<v>_arm64.deb`, `…_aarch64.AppImage`, `AI_Workspace_Local-<ver>-<rel>.aarch64.rpm` |
| `windows-arm64` | `arm64` | app, NSIS, su `windows-11-arm` | `AI_Workspace_Local_<v>_arm64-setup.exe` |
| `daemon-linux-arm64` | `arm64` | daemon `aiw` (llama.cpp incorporato, CPU) | `aiw-<v>-linux-arm64` |
| `daemon-windows-arm64` | `arm64` | daemon `aiw`, CPU | `aiw-<v>-windows-arm64.exe` |
| `cuda-linux` | `cuda` | daemon con `llama-cuda`, CUDA 12.8 | `aiw-<v>-linux-x64-cuda12` |
| `cuda-windows` | `cuda` | daemon con `llama-cuda`, CUDA 12.8 | `aiw-<v>-windows-x64-cuda12.exe` |
| `vulkan-linux` | `vulkan` | daemon con `llama-vulkan`, su `ubuntu-latest` (glibc recente) | `aiw-<v>-linux-x64-vulkan` |
| `vulkan-windows` | `vulkan` | daemon con `llama-vulkan`, SDK Vulkan di LunarG | `aiw-<v>-windows-x64-vulkan.exe` |

`extras` = `linux` + `arm64` + `cuda` + `vulkan`; `all` = `default` + `extras`. `<v>` è la versione senza la `v` del tag. I nomi del daemon cominciano
per `aiw-`: l'aggiornamento dall'app li salta (vedi §6). Tutti i file entrano in `SHA256SUMS.txt`.

I target nuovi (le gambe Linux e arm64 dell'app e tutti i daemon arm64, CUDA e Vulkan) sono **facoltativi**: se falliscono il lavoro è rosso
ma la release parte lo stesso, senza quei file. Una volta provati e stabili si può togliere `"optional": true` dal
catalogo in `scripts/ci/select-targets.sh` (e `continue-on-error` dai lavori `daemon-arm64`, `cuda` e `vulkan`).

Con *Run workflow* e un `tag`, selezionare solo alcuni target pubblica comunque una release con quei soli file: è possibile
ma di solito non è quello che vuoi. Per provare un target nuovo lascia `tag` vuoto.

### Provare un target nuovo senza fare una release

*Actions → Release → Run workflow*, `tag` **vuoto**, `targets` = il target (per esempio `windows-arm64`). Gli artefatti
(`aiw-desktop-<runner>`, `aiw-daemon-<runner>`) si scaricano dalla pagina dell'esecuzione. Le pull request che toccano il
flusso eseguono già `release-check.yml` (solo `cargo check`, senza impacchettare né provare CUDA).

### Windows arm64

- Runner `windows-11-arm`; build nativa (niente cross-compilazione: llama.cpp compila con CMake e MSVC, e il crate
  `llama-cpp-sys-2` esegue `bindgen`, che carica `libclang.dll` nel processo di build, quindi serve una libclang **arm64**).
- `scripts/ci/setup-windows-arm64.ps1` controlla cosa c'è già e installa solo ciò che manca: rustup per aarch64, LLVM 19.1.7
  nella versione `woa64` (nativa arm64) se non c'è una `libclang.dll` arm64, CMake e Ninja (wheel pip) se mancano.
- Accorgimenti ereditati dalle build Windows precedenti: `pwsh` (il `link.exe` della bash di Git oscura quello di MSVC),
  generatore **Ninja** (con quello di Visual Studio l'installazione dei sotto-progetti di llama.cpp falliva), cartella di
  build corta `C:\t` per il daemon (i sotto-progetti CMake superano `MAX_PATH` sotto `D:\a\…`). L'app usa la cartella
  normale, come il lavoro x64.
- llama.cpp incorporato: **solo CPU**. `computer-use` non è attivato nella build arm64 (lo è in x64): `xcap` e l'input
  sintetico non sono mai stati compilati per arm64; si aggiunge a `args` dopo una prima build verde.
- Solo NSIS: Tauri non produce l'MSI per arm64 (e nemmeno la CI x64 lo fa).
- La firma (PFX o Azure) usa gli stessi passi, e gli stessi segreti, del lavoro x64.

### Linux arm64

- Runner `ubuntu-22.04-arm` (come l'x64 di una volta usava 22.04: la build contro la glibc 2.35 gira anche su distribuzioni
  più vecchie). Si possono usare anche i runner `ubuntu-24.04-arm`, con pacchetti più nuovi e glibc 2.39.
- Niente cross-compilazione di GTK/WebKit: build nativa. `computer-use` escluso come su ogni Linux (servono header PipeWire
  più nuovi di quelli di Ubuntu 22.04).
- Il `.rpm`: RPM vieta il `-` nella versione e il CLI Tauri 2.12 pretende una versione SemVer, quindi la tilde
  (`0.5.0~beta.8`) viene rifiutata prima del bundler (era la causa del passo che «riusciva» senza produrre il file, #6).
  La pre-release va nel tag `release`: `0.5.0-beta.10` diventa versione `0.5.0`, release `0.beta.10`
  (`AI_Workspace_Local-0.5.0-0.beta.10.<arch>.rpm`), che si ordina prima della finale `0.5.0-1`. Il passo non è più
  `continue-on-error`: controlla che il file esista e che versione e release siano quelle attese. L'app riporta comunque la
  versione vera (viene da Cargo). Quando uscirà una 2.x del CLI con la conversione `~` (tauri-apps/tauri#16336) si potrà
  tornare alla tilde. Provato su GitHub il 14 novembre 2026 (esecuzione 43606831203, `linux-x64` e `linux-arm64`).
- Costi: i runner arm standard di GitHub (`ubuntu-22.04-arm`, `windows-11-arm`) sono gratuiti per i repository
  **pubblici** (`update.rs` parla di «repository pubblico»; l'indirizzo è `github.com/davides93/ai-workspace-local`). Se il
  repository diventasse privato, i minuti arm si pagano a tariffa propria: conferma la visibilità prima di attivare i gruppi
  `arm64` sui tag.

### CUDA

Il daemon `aiw` con `--features llama-cuda` (il motore llama.cpp incorporato con CUDA), per Linux x64 (`ubuntu-22.04`) e
Windows x64 (`windows-2022`: CUDA 12.8 supporta Visual Studio 2022; sull'immagine `windows-latest` c'è un Visual Studio più
nuovo che nvcc non accetta).

- Il toolkit si installa con `Jimver/cuda-toolkit` (metodo `network`, solo i sotto-pacchetti `nvcc`, runtime e cuBLAS).
- Versione di CUDA: **12.8.1** (famiglia «CUDA 12»: per questo `cuda12` nel nome del file). Architetture compilate: codice
  reale per 7.5 (Turing), 8.0 e 8.6 (Ampere), 8.9 (Ada); PTX per 6.1 (Pascal) e 9.0 (Hopper e Blackwell, compilati al primo
  uso). Cambiale con `CMAKE_CUDA_ARCHITECTURES` nel lavoro `cuda`.
- **Non si distribuisce nessun file NVIDIA.** Su **Linux** cudart e cuBLAS sono collegati staticamente (lo fa
  `llama-cpp-sys-2`): serve solo il **driver NVIDIA** (`libcuda.so.1`), versione 525 o più nuova. Su **Windows** il crate li
  collega dinamicamente: oltre al driver servono `cudart64_12.dll`, `cublas64_12.dll` e `cublasLt64_12.dll`, che si
  ottengono installando il **CUDA Toolkit 12.x** (o mettendo quelle DLL accanto a `aiw.exe`). Lo scrivono anche le note
  della release (vedi il modello). Il lavoro controlla che su Linux il binario non dipenda da `libcudart` o `libcublas`
  condivise e fallisce se è così.
- Il driver NVIDIA deve essere abbastanza nuovo per CUDA 12.x (le note NVIDIA sulla «compatibilità tra versioni minori»
  dicono da quale serie in poi un driver esegue programmi costruiti con CUDA 12.8: verificale prima di scrivere un numero
  nelle note). Con un driver troppo vecchio CUDA non si inizializza; cosa fa allora il motore (errore o CPU) non è provato.
- Resta il caso «chi ha una GPU NVIDIA e non vuole CUDA»: usa il daemon normale o un runtime esterno.
- Se un giorno si vorranno distribuire le DLL (la licenza CUDA le elenca tra i file ridistribuibili) è una decisione da
  prendere e documentare, non un effetto collaterale della build.
### Vulkan

Il daemon `aiw` con `--features llama-vulkan`, Linux x64 (`ubuntu-latest`: glibc recente, come nell'ultima versione verificata) e
Windows x64 (`windows-latest`). Il lavoro `vulkan` è il vecchio `daemon-gpu`, tolto con il commit `3cb23ae` (#54) e ripristinato
come facoltativo (#7). Provato su GitHub il 14 novembre 2026 (esecuzione 43606831203, entrambe le gambe verdi, come
nell'esecuzione 42513812773 che aveva dimostrato la correzione).

- Windows: SDK Vulkan di LunarG scaricato con controllo che sia un eseguibile vero, Ninja al posto del generatore Visual Studio
  e cartella di build corta (`C:\t`) perché i progetti CMake annidati di llama.cpp superano MAX_PATH; build da `pwsh`, perché il
  `link.exe` di Git bash oscura quello di MSVC.
- Non si distribuisce nulla dell'SDK: il loader Vulkan è quello del sistema (lo porta il driver della GPU).
- Mai eseguito su una GPU vera: la build prova solo che compila e collega.
- L'app desktop con CUDA (`--features cuda` in `apps/desktop`) non è costruita da nessun flusso.

### Non costruiti

- **Android x86_64 e armeabi-v7a**: non sono in un flusso di release. L'app Android esce solo da `store.yml` (AAB per
  Google Play, `--target aarch64`), non da una matrice: aggiungere ABI cambierebbe il pacchetto che va ai tester
  (x86_64 serve solo agli emulatori; armeabi-v7a è a 32 bit, senza `dotprod`/`i8mm` e con poco spazio di indirizzi per i
  modelli). Il crate `llama-cpp-sys-2` conosce quelle architetture, ma nessuno le ha mai compilate qui. Per provarle in locale:
  `npx tauri android build --target x86_64` in `apps/desktop`.
- **Linux x64** e il daemon x64/macOS: l'app Linux x64 e il daemon `aiw` non sono più pubblicati dalla release (tolti con
  il commit `3cb23ae`); `ci.yml` compila solo la shell Linux. Si recuperano dal flusso di allora se servono.

## 4. Firma

Con i segreti configurati (`docs/release/FIRMA.md`): macOS Developer ID + notarizzazione; Windows con PFX o Azure Trusted
Signing, scelto dai segreti presenti. Senza segreti la release esce non firmata e il lavoro lo scrive in un avviso. I
lavori arm64 usano gli stessi passi di Windows. Nessun segreto si stampa nei log: passano solo da variabili `env` dei passi.
I daemon (`aiw-*`) non sono firmati in nessun caso.

## 5. Dopo la pubblicazione

1. Controlla la pagina della release: i file attesi (tabella sopra), `SHA256SUMS.txt`, le note.
2. Gli store (`store.yml`) hanno i loro tempi: TestFlight elabora la build, Google Play pubblica sul track `alpha`.
3. Aggiorna `docs/STATUS.md` (§2.7 e §3) con ciò che è stato **davvero provato**.

### Pulizie a mano

- **Release vuota o a metà** (una bozza rimasta dopo un errore, o una release senza file): se è ancora una bozza si
  cancella dalla pagina *Releases* o con `gh release delete vX.Y.Z --yes`. Se era stata pubblicata, il tag e la release sono
  bloccati (release immutabili): non si riusano; si corregge con la versione successiva (per esempio da `beta.8` a `beta.9`).
- **Bozza di una prova**: *Run workflow* con `tag` vuoto non crea nessuna release; con un `tag` crea sempre la bozza e la pubblica.
- **Artefatti vecchi**: gli artefatti dei lavori scadono da soli (predefinito 90 giorni).

## 6. Canali di aggiornamento e nomi dei file

L'app controlla `releases` del repository su GitHub (`crates/aiw-engine/src/update.rs`, le ultime 30, senza le bozze) e
scarica l'installer verificandolo con `SHA256SUMS.txt` (i nomi in quel file devono essere quelli dei file della release:
li scrive `publish` dai file già rinominati).

- Il canale di una versione viene dal suffisso: nessuno = `stable`, `-rc.N` = `rc`, `-beta.N` = `beta`, altro = `alpha`.
  Un canale riceve anche le versioni più stabili, e vince la più nuova.
- Impostazione `update.channel`: `auto` segue la versione **installata** (chi ha una beta resta sulle beta); `stable`,
  `beta`, `alpha` forzano il canale. **Chi ha una versione stabile con `auto` non riceve le beta**, e finché tutte le release
  sono beta una stabile non esiste ancora: per ricevere le beta si imposta `beta`.
- Quale file scarica `pick_asset`: macOS prende il `.dmg` (universale, vale per entrambe le architetture); Windows prende
  prima l'`.msi` con l'architettura giusta, poi `-setup.exe` con l'architettura, poi un `.msi` qualsiasi, poi un `.exe` con
  l'architettura; Linux prende l'`.AppImage` con l'architettura. L'architettura si riconosce dal nome (`aarch64`/`arm64`
  contro `x86_64`/`amd64`/`x64`). Per questo i file nuovi si comportano così:
  `…_arm64-setup.exe` è scelto solo da Windows arm64 (non contiene né `x64` né `x86_64`);
  `…_aarch64.AppImage` è scelto da Linux arm64 (prima di questo target non c'era e l'aggiornamento mostrava solo la pagina
  della release); i `.deb` e i `.rpm` non si scaricano mai dall'app; i file `aiw-…` (daemon arm64 e CUDA) **sono sempre
  saltati**, perché `pick_asset` ignora ogni nome che comincia per `aiw-`. Quindi chiamare un installer `aiw-…` lo nasconderebbe.
- Cosa riceve chi, con `auto`, `stable` e `beta` (provato da un test in `update.rs`, con release `v0.4.1`, `v0.5.0-beta.8`,
  `v0.5.0-beta.9`, `v0.5.0-rc.1`, `v0.6.0-alpha.1`; ogni canale vede anche i più stabili, vince la versione più alta, mai un downgrade):

  | Installato | `auto` | `stable` | `beta` |
  |---|---|---|---|
  | stabile (0.4.0) | `v0.4.1` (non vede beta, rc né alpha) | `v0.4.1` | `v0.5.0-rc.1` |
  | stabile aggiornata (0.4.1) | niente | niente | `v0.5.0-rc.1` |
  | beta (0.5.0-beta.8) | `v0.5.0-rc.1` (beta, rc e stabili, mai alpha) | niente (non scende alla 0.4.1) | `v0.5.0-rc.1` |
  | rc (0.5.0-rc.1) | niente | niente | niente |

  Quando esce `v0.5.0`, tutti la ricevono, beta comprese e anche con `stable`. Una stabile con `auto` non riceve l'alpha
  della versione dopo. Il suffisso si riconosce solo come `-rc` o `-beta` seguiti da punto o da niente: `-beta8` conta come alpha.
- Se una release non ha un installer per la piattaforma, l'app la segnala lo stesso (con il collegamento alla pagina).

## 7. Cosa non è stato provato

- Nessuno dei lavori nuovi (`select`, `daemon-arm64`, `cuda`, le gambe arm64 di `desktop`, `release-check.yml`) è stato
  eseguito su GitHub: sono stati controllati solo come YAML, con `actionlint` e con il `--self-test` di `select-targets.sh`.
- Da verificare alla prima esecuzione: che l'immagine `windows-11-arm` abbia `pwsh`, i componenti ARM64 di Visual Studio e
  (altrimenti) che gli installer di LLVM/CMake/Ninja funzionino; che Tauri produca NSIS per arm64 e AppImage per aarch64; i
  nomi dei sotto-pacchetti CUDA di `Jimver/cuda-toolkit` e che la CUDA 12.8 sia installabile con quel metodo; che il
  collegamento a `-lcuda` trovi lo stub; i tempi (la compilazione di ggml-cuda è la più lunga) e che bastino 120 minuti.
- Nessun binario CUDA è mai stato eseguito su una GPU NVIDIA.
