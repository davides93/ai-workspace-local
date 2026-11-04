# ADR 0005 · Policy separata dalla sandbox, `ask` di default

- Stato: accettata · 2026-11-01

## Contesto
Tutti gli agenti maturi analizzati separano la **decisione** (modalità e regole: chiedere,
consentire o negare) dal **contenimento** (sandbox del sistema operativo: bwrap/seccomp,
Seatbelt). Antigravity aggiunge un *policy guardian* deterministico; Perplexity un audit trail e
un kill switch.

## Decisione
- `PolicyEngine`: modalità (`plan`, `ask`, `auto-edit`, `auto`, `yolo`) + regole `tool(pattern)`
  con `allow`/`ask`/`deny` (vince la più specifica, `deny` prevale).
- `SandboxProfile` (`read-only`, `workspace-write`, `off`) applicato a shell e processi figli.
- Ogni decisione ed esecuzione finisce nell'audit; `POST /api/kill` ferma tutto.

## Conseguenze
- (+) Modalità autonome sicure perché contenute dalla sandbox.
- (−) Su Windows la sandbox completa (AppContainer/token ristretto) è in roadmap: per ora `ask` è il default anche in `auto`.
