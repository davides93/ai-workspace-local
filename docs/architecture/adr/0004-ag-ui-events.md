# ADR 0004 · AG-UI 1.0 come formato degli eventi dei run

- Stato: accettata · 2026-11-01

## Decisione
Gli eventi dei run usano i tipi AG-UI 1.0 (`RUN_STARTED`, `TEXT_MESSAGE_START/CONTENT/END`,
`REASONING_*`, `TOOL_CALL_START/ARGS/END/RESULT`, `STATE_SNAPSHOT`, `SUBAGENT_*`,
`RUN_FINISHED`, `RUN_ERROR`, `CUSTOM`). Le estensioni specifiche (approvazioni, todo, consumo
di token) usano `CUSTOM` con nomi `aiw.*`.

## Conseguenze
- (+) Compatibilità con CopilotKit e con le UI A2UI e MCP Apps; replay dei run dagli eventi salvati.
- (−) Alcuni concetti (approvazioni) non hanno ancora un evento standard: usiamo `CUSTOM`.
