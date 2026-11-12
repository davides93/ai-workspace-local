## Cosa e perché

<!-- Una o due frasi: che cosa cambia e quale problema risolve. -->

## Come l'ho verificato

<!-- Comandi eseguiti e risultato (cargo test -p …, npm run typecheck, npm test, npm run test:smoke), prove a mano. -->

## Provato su dispositivo?

- [ ] Sì: <!-- quale dispositivo/sistema, quale versione -->
- [ ] No: <!-- ciò che è «scritto» ma non «provato» va detto qui e in docs/STATUS.md -->

## Controlli

- [ ] Test aggiunti o aggiornati (o spiegato perché non servono)
- [ ] `cargo fmt --all --check` e `cargo clippy --all-targets` puliti (se ho toccato Rust); `npm run typecheck` e `npm test` (se ho toccato `apps/ui`)
- [ ] Testi per l'utente in italiano **e** in inglese nel catalogo i18n (`apps/ui/src/shared/i18n/`)
- [ ] `CHANGELOG.md`, sezione «Non rilasciato», aggiornato
- [ ] `docs/STATUS.md` aggiornato e onesto («scritto» ≠ «provato»)
- [ ] Nessun segreto, token o percorso personale nel codice, nei log o nelle schermate
