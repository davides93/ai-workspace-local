# Firma del codice: cosa devi creare tu

Il flusso `.github/workflows/release.yml` sa già firmare il DMG macOS e l'installer Windows, ma **non fa nulla finché i
segreti non esistono**: senza, la release esce come prima (non firmata) e il lavoro scrive solo un avviso. Questa pagina
elenca i segreti da creare, dove si prendono e come controllare il risultato.

I nomi delle variabili sono quelli di Tauri 2 (verificati sulla documentazione: «Distribute > Sign» e «Environment
Variables»). I segreti vanno in GitHub: Settings > Secrets and variables > Actions > New repository secret.

Costi e tempi: l'Apple Developer Program costa 99 USD l'anno (approvazione in 1-2 giorni); per Windows serve un
certificato di firma (da 100-400 USD l'anno, con verifica dell'identità) oppure Azure Trusted Signing (circa 10 USD al
mese, ma l'iscrizione è limitata a organizzazioni e persone di alcuni Paesi, con verifica dell'identità).

## macOS: Developer ID e notarizzazione

Serve un account Apple Developer. Non confonderlo con la firma dello store (`store.yml`): quella usa certificati di altro
tipo («Apple Distribution» e «Mac Installer Distribution»). Qui serve un certificato **Developer ID Application**.

| Segreto | Cosa è | Dove lo prendi |
| --- | --- | --- |
| `APPLE_CERTIFICATE` | il certificato Developer ID Application con la chiave privata (`.p12`), in base64 | developer.apple.com > Certificates > «Developer ID Application» (crealo con una richiesta fatta da Accesso Portachiavi). Poi in Accesso Portachiavi: tasto destro sul certificato > Esporta > `.p12` con una password. Converti con `openssl base64 -A -in Certificato.p12 -out certificato.txt` e incolla il contenuto del file. |
| `APPLE_CERTIFICATE_PASSWORD` | la password scelta nell'esportazione del `.p12` | la scegli tu in quel momento |
| `APPLE_SIGNING_IDENTITY` | il nome del certificato, per esempio `Developer ID Application: Nome Cognome (ABCDE12345)` | `security find-identity -v -p codesigning` sul Mac dove hai installato il certificato |
| `APPLE_ID` | l'indirizzo email dell'account Apple | l'account Apple Developer |
| `APPLE_PASSWORD` | una **password specifica per app** (non quella dell'account) | account.apple.com > Accesso e sicurezza > Password specifiche per app |
| `APPLE_TEAM_ID` | l'identificativo del team (10 caratteri) | developer.apple.com/account > Dettagli dell'iscrizione. Se hai già configurato `store.yml` esiste già con questo nome: è lo stesso valore. |

Come si comporta il flusso:

- I primi tre (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`) attivano la firma. Mancando
  anche uno solo, il DMG esce non firmato.
- Gli ultimi tre (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) attivano la notarizzazione. Se mancano, il DMG viene
  firmato ma non notarizzato, e il lavoro lo segnala con un avviso (macOS lo mostrerà comunque con un avvertimento).
- Con la firma attiva la build usa `apps/desktop/src-tauri/tauri.devid.conf.json` (hardened runtime con
  `Entitlements.devid.plist`, che oggi contiene solo il microfono per la dettatura).
- In alternativa alla coppia `APPLE_ID` e `APPLE_PASSWORD`, Tauri accetta una chiave API di App Store Connect
  (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`). Il flusso non la usa ancora.

## Windows: due modi, si sceglie dai segreti presenti

Se sono presenti i segreti di Azure si usa Azure; altrimenti, se c'è il PFX, si usa il PFX; altrimenti non si firma.
Se i segreti di un metodo sono solo in parte presenti, il lavoro lo dice con un avviso e non firma.

### A. Certificato PFX

| Segreto | Cosa è | Dove lo prendi |
| --- | --- | --- |
| `WINDOWS_CERTIFICATE` | il certificato di firma del codice (`.pfx`) in base64 | lo emette un'autorità di certificazione (DigiCert, Sectigo, SSL.com...) dopo la verifica dell'identità. Dalle regole del settore (CA/Browser Forum) la chiave privata deve stare su un token hardware o in un servizio cloud, quindi molti certificati nuovi non si possono esportare in `.pfx`: chiedi alla CA un certificato esportabile oppure usa il metodo B. Per convertire: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("certificato.pfx"))` in PowerShell. |
| `WINDOWS_CERTIFICATE_PASSWORD` | la password del `.pfx` | la scegli tu in fase di esportazione |

Facoltativa: la variabile (non un segreto) `WINDOWS_TIMESTAMP_URL`, per cambiare il server di marcatura temporale
(predefinito `http://timestamp.digicert.com`). Il flusso importa il certificato, ne legge l'impronta e la passa a Tauri
(`certificateThumbprint`, `digestAlgorithm` `sha256`, `timestampUrl`).

### B. Azure Trusted Signing (oggi «Artifact Signing»)

Si crea nel portale Azure un account di firma e un profilo di certificato (con la verifica dell'identità), poi una
registrazione di app (App Registration) con il ruolo «Trusted Signing Certificate Profile Signer» sul profilo.

| Segreto | Cosa è | Dove lo prendi |
| --- | --- | --- |
| `AZURE_CLIENT_ID` | l'ID applicazione della registrazione di app | portale Azure > Microsoft Entra ID > Registrazioni app |
| `AZURE_CLIENT_SECRET` | un segreto client di quella registrazione | stessa pagina > Certificati e segreti > Nuovo segreto client (si vede una volta sola) |
| `AZURE_TENANT_ID` | l'ID della directory | stessa pagina, «ID directory (tenant)» |
| `AZURE_SIGNING_ENDPOINT` | l'indirizzo dell'endpoint, per esempio `https://weu.codesigning.azure.net` | pagina dell'account di firma, secondo l'area scelta |
| `AZURE_SIGNING_ACCOUNT` | il nome dell'account di firma | portale Azure |
| `AZURE_SIGNING_PROFILE` | il nome del profilo di certificato | dentro l'account di firma |

Il flusso installa `artifact-signing-cli` (`cargo install artifact-signing-cli --locked`, senza versione fissata) e passa
a Tauri `bundle.windows.signCommand` con `-e`, `-a`, `-c` e `-d`.

## Come verificare una build firmata

Il flusso verifica da solo, dopo la build, e **fallisce** se la firma era configurata ma non è valida. Per controllare a
mano gli artefatti scaricati dalla release:

macOS (sul DMG montato, o sull'app):

```sh
codesign --verify --deep --strict --verbose=2 "AI Workspace Local.app"
codesign -dv --verbose=2 "AI Workspace Local.app"       # Authority: Developer ID Application..., flags=0x10000(runtime)
spctl --assess --type execute --verbose=2 "AI Workspace Local.app"   # accepted, source=Notarized Developer ID
xcrun stapler validate "AI Workspace Local.app"          # The validate action worked!
```

Windows (PowerShell):

```powershell
Get-AuthenticodeSignature .\AI_Workspace_Local_*_setup.exe | Format-List Status, SignerCertificate, TimeStamperCertificate
# Status deve essere Valid; in Esplora risorse: Proprietà > Firme digitali
```

Se un controllo fallisce, la release non parte (il lavoro si ferma prima del caricamento degli artefatti).

## Cosa non è stato provato

Il flusso non è mai stato eseguito con i segreti veri: sono stati controllati solo la sintassi del file e il percorso «senza
segreti» per costruzione. Da vedere alla prima release firmata: che l'hardened runtime non blocchi il programma di
supporto di Apple Foundation Models né i runtime scaricati (nel caso servono altri entitlements in
`Entitlements.devid.plist`), che `signtool` sia disponibile sul runner Windows con il PFX, e il nome esatto del ruolo
Azure. Le impronte dei certificati e i segreti non vanno mai scritti nel repository né nei log.
