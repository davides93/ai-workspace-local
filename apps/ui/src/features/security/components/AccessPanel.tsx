import { useState } from "react";
import { useStoredUiMode } from "../../../shared/lib/uiMode";
import type { ApiTokenCreated, ApiTokenInfo } from "../../../shared/api/types";
import { Button, CopyField, CodeBlock, Dialog, Field, Input, Panel, Select } from "../../../shared/ui";
import { useApiTokens, useCreateToken, useRevokeToken, useRotateToken } from "../api";
import { describeExpiry, describeLastUse, EXPIRY_CHOICES, exampleFor, tokenState } from "../model/tokens";

type Confirm = { token: ApiTokenInfo; action: "revoke" | "rotate" };

/**
 * Who can reach this computer: the paired devices (each with its own token, «Scollega» revokes it at
 * once) and, in developer mode, the tokens of apps of the person (OpenAI/Anthropic-compatible `/v1`).
 */
export function AccessPanel() {
  const mode = useStoredUiMode();
  const tokens = useApiTokens();
  const revoke = useRevokeToken();
  const rotate = useRotateToken();
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [shown, setShown] = useState<ApiTokenCreated | null>(null);
  const [creating, setCreating] = useState(false);
  const all = tokens.data ?? [];
  const devices = all.filter((t) => t.kind === "device");
  const apps = all.filter((t) => t.kind === "gateway");
  const owner = all.find((t) => t.kind === "owner");
  const developer = mode === "developer";

  const row = (t: ApiTokenInfo, label: string) => {
    const state = tokenState(t);
    return (
      <li className="list-item" key={t.id}>
        <div className="grow">
          <b>{t.name}</b> {state !== "active" && <span className="faint">({state === "revoked" ? "revocato" : "scaduto"})</span>}
          <div className="faint mono text-xs">{t.prefix} · {describeLastUse(t)} · {describeExpiry(t)}</div>
        </div>
        {state === "active" && (
          <>
            {developer && <Button onClick={() => setConfirm({ token: t, action: "rotate" })}>Ruota</Button>}
            <Button variant="danger" onClick={() => setConfirm({ token: t, action: "revoke" })}>{label}</Button>
          </>
        )}
      </li>
    );
  };

  return (
    <div className="stack">
      <Panel icon="device" title="Dispositivi accoppiati a questo computer" padded={false}>
        <ul className="list" aria-label="Dispositivi accoppiati a questo computer">
          {devices.map((t) => row(t, "Scollega"))}
          {devices.length === 0 && <li className="list-item faint text-sm">Nessun dispositivo accoppiato a questo computer.</li>}
        </ul>
      </Panel>
      {developer && (
        <Panel icon="key" title="Token per le app" padded={false} actions={<Button variant="primary" onClick={() => setCreating(true)}>Nuovo token per un&apos;app</Button>}>
          <ul className="list" aria-label="Token per le app">
            {apps.map((t) => row(t, "Revoca"))}
            {apps.length === 0 && <li className="list-item faint text-sm">Nessun token per app. Creane uno per usare AI Workspace da un&apos;altra app compatibile OpenAI.</li>}
          </ul>
          {owner && (
            <p className="faint text-xs panel-body">
              Il token di questo computer ({owner.prefix}) serve all&apos;app e alla riga di comando qui e non si revoca da qui: per cambiarlo ferma il servizio, cancella il file «token» nella cartella dei dati e riavvia.
            </p>
          )}
        </Panel>
      )}
      {confirm && (
        <Dialog
          title={confirm.action === "revoke" ? (confirm.token.kind === "device" ? "Scollega il dispositivo" : "Revoca il token") : "Ruota il token"} size="sm" onClose={() => setConfirm(null)}
          footer={<>
            <Button onClick={() => setConfirm(null)}>Annulla</Button>
            <Button
              variant="danger" disabled={revoke.isPending || rotate.isPending}
              onClick={() => {
                const t = confirm.token;
                if (confirm.action === "revoke") revoke.mutate(t, { onSettled: () => setConfirm(null) });
                else rotate.mutate(t, { onSuccess: (c) => { setConfirm(null); setShown(c); }, onSettled: () => setConfirm(null) });
              }}
            >{confirm.action === "revoke" ? (confirm.token.kind === "device" ? "Scollega" : "Revoca") : "Ruota"}</Button>
          </>}
        >
          {confirm.action === "revoke"
            ? <p><b>{confirm.token.name}</b> smette di funzionare subito e le sue connessioni aperte si chiudono. {confirm.token.kind === "device" ? "Per usarlo di nuovo dovrai accoppiarlo con un nuovo codice." : "L'app che lo usa riceverà un errore finché non gliene dai uno nuovo."}</p>
            : <p>Il valore di <b>{confirm.token.name}</b> viene sostituito: quello vecchio smette di funzionare subito e il nuovo si vede una volta sola.</p>}
        </Dialog>
      )}
      {creating && <NewTokenDialog onClose={() => setCreating(false)} onCreated={(c) => { setCreating(false); setShown(c); }} />}
      {shown && (
        <Dialog title="Copia il token adesso" size="md" onClose={() => setShown(null)} footer={<Button variant="primary" onClick={() => setShown(null)}>Fatto</Button>}>
          <p>Questo è l&apos;unico momento in cui il token si vede: AI Workspace ne conserva solo un&apos;impronta e non potrà mostrarlo di nuovo.</p>
          <CopyField label={shown.name} value={shown.token} />
          {shown.kind === "gateway" && <CodeBlock code={exampleFor(shown.token)} />}
        </Dialog>
      )}
    </div>
  );
}

function NewTokenDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (t: ApiTokenCreated) => void }) {
  const create = useCreateToken();
  const [name, setName] = useState("");
  const [days, setDays] = useState("0");
  const [extra, setExtra] = useState<string[]>([]);
  const toggle = (s: string) => setExtra((e) => (e.includes(s) ? e.filter((x) => x !== s) : [...e, s]));
  return (
    <Dialog
      title="Nuovo token per un'app" size="md" onClose={onClose}
      footer={<>
        <Button onClick={onClose}>Annulla</Button>
        <Button
          variant="primary" disabled={create.isPending || !name.trim()}
          onClick={() => create.mutate({ name: name.trim(), kind: "gateway", expires_in_days: Number(days) || undefined, scopes: extra }, { onSuccess: onCreated })}
        >Crea</Button>
      </>}
    >
      <p className="faint">Il token permette solo le richieste compatibili con OpenAI e Anthropic (<span className="mono">/v1</span>): non può cambiare le impostazioni né leggere le tue chat.</p>
      <Field label="Nome dell'app" htmlFor="tok-name">
        <Input id="tok-name" value={name} onChange={(e) => setName(e.target.value)} placeholder="Per esempio: editor, script di casa" autoComplete="off" />
      </Field>
      <Field label="Scadenza" htmlFor="tok-exp">
        <Select id="tok-exp" value={days} onChange={(e) => setDays(e.target.value)}>
          {EXPIRY_CHOICES.map((c) => <option key={c.days} value={String(c.days)}>{c.label}</option>)}
        </Select>
      </Field>
      <div className="row">
        <label><input type="checkbox" checked={extra.includes("mcp")} onChange={() => toggle("mcp")} /> Anche il server MCP</label>
        <label><input type="checkbox" checked={extra.includes("a2a")} onChange={() => toggle("a2a")} /> Anche gli agenti (A2A)</label>
      </div>
      {create.error && <p role="alert" className="danger">{(create.error as Error).message}</p>}
    </Dialog>
  );
}
