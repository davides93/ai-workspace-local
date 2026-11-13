<!--
Modello delle note di una release: copialo in docs/release-notes/vX.Y.Z.md (il nome è il tag).
`release.yml` usa quel file come testo della release; senza, la pubblicazione fallisce.
Cancella i commenti e le righe dei file che questa release non contiene.
Scrivi solo ciò che è vero: «scritto» non vuol dire «provato» (vedi CONTRIBUTING.md).
-->
# AI Workspace Local X.Y.Z-beta.N (pre-release)

- **Titolo:** una riga per novità, scritta per chi usa l'app.

## Verifiche e limiti

Cosa è stato provato e dove (CI, dispositivo reale, modelli veri) e cosa no.

## File di questa release

Tutti i file hanno il loro hash in `SHA256SUMS.txt`. Le app non sono firmate se la release è uscita senza i segreti di firma.

| File | Per cosa |
| --- | --- |
| `AI_Workspace_Local_X.Y.Z_universal.dmg` | macOS (Apple Silicon e Intel) |
| `AI_Workspace_Local_X.Y.Z_x64-setup.exe` | Windows x64 |
| `AI_Workspace_Local_X.Y.Z_arm64-setup.exe` | Windows arm64 (facoltativo, mai provato su un dispositivo) |
| `AI_Workspace_Local_X.Y.Z_arm64.deb`, `…_aarch64.AppImage`, `…-1.aarch64.rpm` | Linux arm64 (facoltativi) |
| `aiw-X.Y.Z-linux-arm64`, `aiw-X.Y.Z-windows-arm64.exe` | daemon e CLI `aiw` per arm64, solo CPU (facoltativi) |
| `aiw-X.Y.Z-linux-x64-cuda12`, `aiw-X.Y.Z-windows-x64-cuda12.exe` | daemon e CLI `aiw` con CUDA 12 (facoltativi) |
| `SHA256SUMS.txt` | SHA-256 di ogni file |

**CUDA.** I file `…-cuda12` sono costruiti con CUDA 12.8. Servono una GPU NVIDIA e il **driver NVIDIA** installato.
Su Linux non serve altro (le librerie CUDA sono incluse nel programma). Su Windows servono anche le DLL di
CUDA 12 (`cudart64_12.dll`, `cublas64_12.dll`, `cublasLt64_12.dll`): installa il CUDA Toolkit 12.x oppure mettile accanto a
`aiw.exe`. Nessun file NVIDIA è distribuito con la release.

Build store: **NNNN** (macOS e iOS su TestFlight, Android sul test chiuso alpha).
