# Il computer dell'agente (container)

Parte della strada A per gli agenti proattivi in stile OpenBot/OpenDots: il nostro agente guida un container, non il contrario. Questo
primo pezzo è **un browser** (Chromium) in un container, uno per agente, con la **cartella di lavoro dell'agente** montata come
`/workspace`. La shell nel container, la vista sullo schermo dal telefono e l'interfaccia OpenDots opzionale non ci sono ancora.

## Come si usa

Serve Docker (o Podman) acceso e con Compose: `aiw containers` lo dice, e in modalità Semplice la voce «Un computer per i tuoi agenti» del menu
guida il controllo.

| Cosa | Come |
|---|---|
| Avviare (la prima volta costruisce l'immagine: minuti, scarica `debian:bookworm-slim` e Chromium) | `aiw containers up <agente>`, oppure `POST /api/containers/computers/{agente}/up` (ambito `full`, risponde subito `starting`) |
| Vedere a che punto è | `aiw containers status <agente>`, oppure `GET /api/containers/computers/{agente}`: `absent`, `starting`, `running` (con `cdp_url`), `stopped`, `error` (con il motivo) |
| Fermare e togliere il container | `aiw containers down <agente>`, oppure `POST /api/containers/computers/{agente}/down`. **La cartella dell'agente e il profilo del browser restano** |

L'avvio non parte mai da solo e nessun modello lo può chiamare: è un'azione del proprietario (rotte `full`, un telefono accoppiato riceve 403).
L'id dell'agente deve avere solo lettere minuscole, cifre, `-` e `_` (nome di progetto Compose e di cartella).

## Cosa viene creato

In `<dati>/agents/<id>/computer/`: `Dockerfile` e `compose.yml` (copie di `crates/aiw-engine/assets/agent-computer/`, riscritte a ogni avvio), e
la cartella `<dati>/agents/<id>/workspace` se manca (la stessa degli agenti isolati, vedi `docs/agenti-isolati.md`). Progetto Compose:
`aiw-computer-<id>`, immagine `aiw/agent-computer:local` costruita sul tuo computer.

## Cosa lo chiude

- Porta di debug del browser pubblicata **solo su `127.0.0.1`** del computer, con porta scelta da Docker (`docker compose port`): nessun altro
  computer della rete la raggiunge, e due agenti non si contendono una porta.
- Utente non root (`1000`, o il proprietario della cartella su Linux), filesystem di sistema in sola lettura, `/tmp` in memoria, tutte le
  capacità tolte, `no-new-privileges`, al massimo 512 processi, 2 GB di memoria e 2 CPU (`AIW_MEM`, `AIW_CPUS` nell'ambiente di chi lancia `aiw`).
- **Nessuna variabile d'ambiente passa nel container**: nessun token di AIW, nessuna chiave. Non c'è `docker.sock`, né `privileged`, né rete dell'host
  (un test lo verifica sul file).
- Il profilo del browser (cookie, accessi) sta in `/workspace/.browser-profile`: sopravvive al riavvio e un altro agente non lo vede.

## Cosa NON è (ancora)

- **Mai provato con un Docker o un Podman che gira**: l'autore aveva Docker installato ma spento. Le righe di comando sono provate contro un
  esecutore finto; la build dell'immagine e l'avvio non sono mai stati eseguiti. Potrebbe servire correggere i pacchetti dell'immagine o le opzioni di Chromium.
- Chromium parte con `--no-sandbox`: la sua sandbox ha bisogno di privilegi che il container non ha; il confine è il container.
- Immagine di base fissata per **etichetta**, non per digest (non ho potuto verificarlo).
- Chiunque sul computer può collegarsi alla porta di debug su loopback: è lo stesso livello di fiducia del collegamento a Chrome (`browser.attach_url`, `docs/browser-live.md`).
- Il browser dell'agente (`browser_*`) **non usa ancora** questo container: oggi `browser.attach_url` si imposta a mano con l'indirizzo di `cdp_url`.
- Nessuna shell nel container (`sandbox.backend = container`), nessuna vista dal telefono, nessun limite di rete per container.
- Windows e macOS: Docker Desktop lavora in una macchina virtuale e i permessi dei volumi vanno provati.
