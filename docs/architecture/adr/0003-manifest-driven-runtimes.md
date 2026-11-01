# ADR 0003 · Runtime descritti da manifest dichiarativi

- Stato: accettata · 2026-11-01

## Contesto
"Tutti i metodi di inferenza locale per ogni device" vuol dire decine di motori in continua
evoluzione. Scrivere un adapter a mano per ognuno non scala.

## Decisione
- Ogni runtime è un manifest JSON: tipo, piattaforme, acceleratori, formati, dialetto API,
  rilevamento, ricetta di avvio con placeholder, health check, schema delle impostazioni,
  suggerimenti di installazione.
- Tre adapter generici (`openai`, `ollama`, `native`) coprono tutti i casi; i manifest utente in
  `~/.aiw/runtimes/` estendono il catalogo senza ricompilare.
- La UI genera i form dalle impostazioni (sottoinsieme di JSON Schema).

## Conseguenze
- (+) Aggiungere un motore costa un file JSON; anche l'utente finale può farlo.
- (−) I motori con API non standard richiedono un adapter `native` o un bridge.
