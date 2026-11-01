# ADR 0002 · Core headless con una sola API su IPC e su HTTP/SSE

- Stato: accettata · 2026-11-01

## Contesto
Codex (app-server JSON-RPC), OpenCode (server HTTP) e Antigravity (ConnectRPC tra Electron e il
language server Go) separano il motore dai client (pattern P01). Vogliamo UI desktop e mobile,
browser in LAN, CLI e app terze.

## Decisione
- `aiw-engine::Workspace` è la facciata unica; `aiw-server` la espone su REST e SSE, la shell
  Tauri tramite comandi IPC con gli stessi nomi e payload.
- La UI usa un'interfaccia `Transport` con tre implementazioni: Tauri, HTTP e Mock (demo).
- Gli stream dei run sono eventi AG-UI (ADR 0004).

## Conseguenze
- (+) La UI può anche girare in un browser collegato al PC (accesso dal telefono senza app).
- (+) Test end-to-end del core via HTTP senza UI.
- (−) Doppio binding (IPC e HTTP) da tenere allineato: mitigato da tipi condivisi e da un solo dispatcher.
