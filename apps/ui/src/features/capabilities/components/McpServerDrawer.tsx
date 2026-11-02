import { useState, type FormEvent, type ReactNode } from "react";
import type { McpServer } from "../../../shared/api/types";
import { splitShellWords } from "../../../shared/lib/tokens";
import { Button, Drawer, Field, Icon, Input, Toggle } from "../../../shared/ui";
import { KeyValueEditor } from "../../../shared/ui/KeyValueEditor";
import { TagInput } from "../../../shared/ui/TagInput";
import {
  draftFromServer, draftToPayload, emptyDraft, isSecretKey, normalizeName, splitCommandLine, validateDraft,
  type McpDraft, type McpPayload, type McpTransportKind,
} from "../model/mcpForm";

const TRANSPORTS: { id: McpTransportKind; label: string }[] = [
  { id: "stdio", label: "stdio (comando locale)" },
  { id: "http", label: "HTTP (server remoto)" },
];

/** A labelled group of controls (not a single input, so the label is text and not a `<label>`). */
function GroupField({ label, hint, error, children }: { label: string; hint?: ReactNode; error?: string; children: ReactNode }) {
  return (
    <div className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="hint">{hint}</span>}
      {error && <span className="field-error" role="alert">{error}</span>}
    </div>
  );
}

/** Add or edit a server: stdio (command, arguments, environment) or http (url, headers). */
export function McpServerDrawer({ server, others, saving, onSave, onClose }: {
  /** The server being edited; `null` adds a new one. */
  server: McpServer | null;
  /** Names of the other servers, for the duplicate check. */
  others: string[];
  saving: boolean;
  onSave: (payload: McpPayload) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<McpDraft>(() => (server ? draftFromServer(server) : emptyDraft()));
  const [submitted, setSubmitted] = useState(false);
  const errors = validateDraft(draft, others);
  const shown = submitted ? errors : {};
  const patch = (p: Partial<McpDraft>) => setDraft((d) => ({ ...d, ...p }));
  const split = draft.transport === "stdio" ? splitCommandLine(draft) : null;
  const renamed = !!server && normalizeName(draft.name) !== server.name && normalizeName(draft.name) !== "";

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    const first = (["name", "command", "url"] as const).find((k) => errors[k]);
    if (first) return void document.getElementById(`mcp-${first}`)?.focus();
    if (Object.keys(errors).length === 0) onSave(draftToPayload(draft));
  };

  return (
    <Drawer
      title={server ? `Modifica ${server.name}` : "Nuovo server MCP"}
      subtitle="I suoi tool diventano disponibili agli agenti come mcp__nome__tool."
      onClose={onClose}
      footer={<>
        <Button onClick={onClose}>Annulla</Button>
        <Button variant="primary" type="submit" form="mcp-form" disabled={saving}>{saving ? "Salvo…" : "Salva e collega"}</Button>
      </>}
    >
      <form id="mcp-form" className="form-grid" onSubmit={submit} noValidate>
        <Field label="Nome" htmlFor="mcp-name" error={shown.name} hint="Senza spazi: gli spazi diventano «_».">
          <Input id="mcp-name" data-autofocus value={draft.name} placeholder="filesystem" aria-invalid={!!shown.name} autoComplete="off" onChange={(e) => patch({ name: e.target.value })} />
        </Field>
        {renamed && (
          <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> Rinominare cambia il prefisso dei tool (mcp__{normalizeName(draft.name)}__…): agenti e regole della policy che li citano vanno aggiornati.</p>
        )}

        <div className="field">
          <span className="field-label" id="mcp-transport-label">Trasporto</span>
          <div className="seg cap-transport" role="group" aria-labelledby="mcp-transport-label">
            {TRANSPORTS.map((t) => (
              <button key={t.id} type="button" aria-pressed={draft.transport === t.id} onClick={() => patch({ transport: t.id })}>{t.label}</button>
            ))}
          </div>
        </div>

        {draft.transport === "stdio" ? (
          <>
            <p className="cap-note cap-note-warn text-sm">
              <Icon name="alert" size={15} />
              <span>Un server stdio è un processo avviato con i tuoi permessi, <b>fuori dalla sandbox</b> degli agenti: avvia solo comandi di cui ti fidi.</span>
            </p>
            <Field label="Comando" htmlFor="mcp-command" error={shown.command} hint={split ? undefined : "Il programma da avviare, senza argomenti."}>
              <Input id="mcp-command" mono value={draft.command} placeholder="npx" aria-invalid={!!shown.command} autoComplete="off" spellCheck={false} onChange={(e) => patch({ command: e.target.value })} />
            </Field>
            {split && (
              <p className="cap-note text-sm">
                <Icon name="info" size={15} />
                <span>Il comando contiene spazi. Se è una riga completa, dividila: i parametri vanno tra gli argomenti.</span>
                <Button size="sm" onClick={() => patch(split)}>Dividi</Button>
              </p>
            )}
            <Field label="Argomenti" htmlFor="mcp-args" hint="Invio aggiunge un argomento (anche incollando una riga). Le virgolette tengono insieme gli spazi: &quot;~/Mie Cartelle&quot;.">
              <TagInput id="mcp-args" label="Argomenti" value={draft.args} onChange={(args) => patch({ args })} parse={splitShellWords} placeholder={'-y @modelcontextprotocol/server-filesystem "~/Mie Cartelle"'} />
            </Field>
            <GroupField label="Variabili d'ambiente" error={shown.env} hint="I valori di chiavi, token e password sono nascosti.">
              <KeyValueEditor
                idPrefix="mcp-env" label="Variabili d'ambiente" rows={draft.env} onChange={(env) => patch({ env })} keyLabel="Variabile" valueLabel="Valore"
                keyPlaceholder="GITHUB_TOKEN" isSecret={isSecretKey} addLabel="Aggiungi variabile" emptyText="Nessuna variabile oltre a quelle dell'app."
              />
            </GroupField>
          </>
        ) : (
          <>
            <Field label="Indirizzo" htmlFor="mcp-url" error={shown.url} hint="Endpoint MCP in streamable HTTP.">
              <Input id="mcp-url" mono value={draft.url} placeholder="http://127.0.0.1:8931/mcp" aria-invalid={!!shown.url} autoComplete="off" spellCheck={false} onChange={(e) => patch({ url: e.target.value })} />
            </Field>
            <GroupField label="Header HTTP" error={shown.headers} hint="Authorization e gli header che contengono token o chiavi sono nascosti.">
              <KeyValueEditor
                idPrefix="mcp-hdr" label="Header HTTP" rows={draft.headers} onChange={(headers) => patch({ headers })} keyLabel="Header" valueLabel="Valore"
                keyPlaceholder="Authorization" valuePlaceholder="Bearer …" isSecret={isSecretKey} addLabel="Aggiungi header" emptyText="Nessun header aggiuntivo."
              />
            </GroupField>
          </>
        )}

        <div className="row row-between nowrap">
          <div className="field">
            <span className="field-label" id="mcp-enabled-label">Collega il server</span>
            <span className="hint">Un server disattivato resta configurato ma non viene collegato.</span>
          </div>
          <Toggle id="mcp-enabled" checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label="Collega il server" />
        </div>
      </form>
    </Drawer>
  );
}
