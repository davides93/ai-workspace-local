import { useState, type FormEvent, type ReactNode } from "react";
import type { McpServer } from "../../../shared/api/types";
import { splitShellWords } from "../../../shared/lib/tokens";
import { SECRET_MASK } from "../../../shared/lib/secrets";
import { Button, Drawer, Field, Icon, Input, KeyValueEditor, TagInput, Toggle } from "../../../shared/ui";
import { useSystem } from "../../../shared/query/system";
import type { McpSandbox } from "../../../shared/api/types";
import {
  MCP_SANDBOX_LABEL, draftFromServer, draftToPayload, emptyDraft, isSecretKey, normalizeName, splitCommandLine, validateDraft,
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
  const system = useSystem();
  const isolationAvailable = system.data?.mcp_sandbox ?? false;
  const [draft, setDraft] = useState<McpDraft>(() => (server ? draftFromServer(server) : emptyDraft(isolationAvailable)));
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
    if (errors.env) return void document.getElementById("mcp-env-v0")?.focus();
    if (errors.headers) return void document.getElementById("mcp-hdr-v0")?.focus();
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
              <span>Un server stdio è un processo avviato sul tuo computer: avvia solo comandi di cui ti fidi. Alla prima partenza di un comando nuovo o modificato ti chiediamo conferma.</span>
            </p>
            <div className="field">
              <span className="field-label" id="mcp-sandbox-label">Isolamento</span>
              <div className="seg" role="group" aria-labelledby="mcp-sandbox-label">
                {(["workspace-write", "read-only", "off"] as McpSandbox[]).map((m) => (
                  <button key={m} type="button" aria-pressed={draft.sandbox === m} disabled={m !== "off" && !isolationAvailable && draft.sandbox !== m}
                    onClick={() => patch({ sandbox: m })}>{MCP_SANDBOX_LABEL[m]}</button>
                ))}
              </div>
              <span className="hint">
                {isolationAvailable
                  ? "Isolato, il server non vede la cartella dei dati dell'app né le cartelle delle credenziali, e non eredita le variabili d'ambiente dell'app (riceve solo quelle che scrivi qui sotto). «Nessun isolamento» lo lascia girare con i tuoi permessi."
                  : "Su questo sistema (Windows) l'isolamento dei server MCP non è disponibile: il server gira con i tuoi permessi."}
              </span>
            </div>
            {draft.sandbox !== "off" && (
              <div className="row row-between nowrap">
                <div className="field">
                  <span className="field-label" id="mcp-sandbox-net-label">Consenti la rete al server</span>
                  <span className="hint">Molti server ne hanno bisogno per parlare con un servizio. Spegnila per uno che lavora solo su file locali.</span>
                </div>
                <Toggle id="mcp-sandbox-net" checked={draft.sandboxNetwork} onChange={(sandboxNetwork) => patch({ sandboxNetwork })} label="Consenti la rete al server" />
              </div>
            )}
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
            <GroupField label="Variabili d'ambiente" error={shown.env} hint="I valori di chiavi, token e password sono nascosti: quelli già salvati restano invariati finché non li cambi.">
              <KeyValueEditor
                idPrefix="mcp-env" label="Variabili d'ambiente" rows={draft.env} onChange={(env) => patch({ env })} keyLabel="Variabile" valueLabel="Valore"
                keyPlaceholder="GITHUB_TOKEN" isSecret={isSecretKey} mask={SECRET_MASK} addLabel="Aggiungi variabile" emptyText="Nessuna variabile oltre a quelle dell'app."
              />
            </GroupField>
          </>
        ) : (
          <>
            <Field label="Indirizzo" htmlFor="mcp-url" error={shown.url} hint="Endpoint MCP in streamable HTTP.">
              <Input id="mcp-url" mono value={draft.url} placeholder="http://127.0.0.1:8931/mcp" aria-invalid={!!shown.url} autoComplete="off" spellCheck={false} onChange={(e) => patch({ url: e.target.value })} />
            </Field>
            <GroupField label="Header HTTP" error={shown.headers} hint="Authorization e gli header che contengono token o chiavi sono nascosti: quelli già salvati restano invariati finché non li cambi.">
              <KeyValueEditor
                idPrefix="mcp-hdr" label="Header HTTP" rows={draft.headers} onChange={(headers) => patch({ headers })} keyLabel="Header" valueLabel="Valore"
                keyPlaceholder="Authorization" valuePlaceholder="Bearer …" isSecret={isSecretKey} mask={SECRET_MASK} addLabel="Aggiungi header" emptyText="Nessun header aggiuntivo."
              />
            </GroupField>
          </>
        )}

        {draft.transport === "http" && (
          <Field label="Client id OAuth (facoltativo)" htmlFor="mcp-client-id" hint="Serve solo se il server di autorizzazione non permette la registrazione automatica. Per l'accesso usa «Accedi» sulla scheda del server.">
            <Input id="mcp-client-id" mono value={draft.oauthClientId} autoComplete="off" spellCheck={false} onChange={(e) => patch({ oauthClientId: e.target.value })} />
          </Field>
        )}

        <div className="row row-between nowrap">
          <div className="field">
            <span className="field-label" id="mcp-sampling-label">Consenti al server di usare il modello</span>
            <span className="hint">Campionamento MCP: il server può chiedere risposte al modello dell'esecuzione in corso. Consuma token del tuo budget (con un tetto per esecuzione) e il testo che invia non è attendibile. Lascialo spento se non ti serve.</span>
          </div>
          <Toggle id="mcp-sampling" checked={!!draft.sampling} onChange={(sampling) => patch({ sampling })} label="Consenti al server di usare il modello" />
        </div>
        {draft.sampling && (
          <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> Un server che non controlli può usare così il tuo modello (e il tuo tempo di calcolo) per i suoi scopi. Ogni richiesta è registrata nel registro delle attività.</p>
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
