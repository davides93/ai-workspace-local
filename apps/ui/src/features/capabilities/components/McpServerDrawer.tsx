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

import { t as tr } from "../../../shared/i18n";
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
      title={server ? tr("capabilities.modifica", { name: server.name }) : tr("capabilities.nuovo_server_mcp")}
      subtitle={tr("capabilities.i_suoi_tool_diventano_disponibili")}
      onClose={onClose}
      footer={<>
        <Button onClick={onClose}>{tr("capabilities.annulla")}</Button>
        <Button variant="primary" type="submit" form="mcp-form" disabled={saving}>{saving ? tr("capabilities.salvo") : tr("capabilities.salva_e_collega")}</Button>
      </>}
    >
      <form id="mcp-form" className="form-grid" onSubmit={submit} noValidate>
        <Field label={tr("capabilities.nome")} htmlFor="mcp-name" error={shown.name} hint={tr("capabilities.senza_spazi_gli_spazi_diventano")}>
          <Input id="mcp-name" data-autofocus value={draft.name} placeholder={tr("capabilities.filesystem")} aria-invalid={!!shown.name} autoComplete="off" onChange={(e) => patch({ name: e.target.value })} />
        </Field>
        {renamed && (
          <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> {" "}{tr("capabilities.rinominare_cambia_il_prefisso_dei", { p: normalizeName(draft.name) })}</p>
        )}

        <div className="field">
          <span className="field-label" id="mcp-transport-label">{tr("capabilities.trasporto")}</span>
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
              <span>{tr("capabilities.un_server_stdio_e_un")}</span>
            </p>
            <div className="field">
              <span className="field-label" id="mcp-sandbox-label">{tr("capabilities.isolamento")}</span>
              <div className="seg" role="group" aria-labelledby="mcp-sandbox-label">
                {(["workspace-write", "read-only", "off"] as McpSandbox[]).map((m) => (
                  <button key={m} type="button" aria-pressed={draft.sandbox === m} disabled={m !== "off" && !isolationAvailable && draft.sandbox !== m}
                    onClick={() => patch({ sandbox: m })}>{MCP_SANDBOX_LABEL[m]}</button>
                ))}
              </div>
              <span className="hint">
                {isolationAvailable ? tr("capabilities.isolato_il_server_non_vede") : tr("capabilities.su_questo_sistema_windows_l")}
              </span>
            </div>
            {draft.sandbox !== "off" && (
              <div className="row row-between nowrap">
                <div className="field">
                  <span className="field-label" id="mcp-sandbox-net-label">{tr("capabilities.consenti_la_rete_al_server")}</span>
                  <span className="hint">{tr("capabilities.molti_server_ne_hanno_bisogno")}</span>
                </div>
                <Toggle id="mcp-sandbox-net" checked={draft.sandboxNetwork} onChange={(sandboxNetwork) => patch({ sandboxNetwork })} label={tr("capabilities.consenti_la_rete_al_server")} />
              </div>
            )}
            <Field label={tr("capabilities.comando")} htmlFor="mcp-command" error={shown.command} hint={split ? undefined : tr("capabilities.il_programma_da_avviare_senza")}>
              <Input id="mcp-command" mono value={draft.command} placeholder={tr("capabilities.npx")} aria-invalid={!!shown.command} autoComplete="off" spellCheck={false} onChange={(e) => patch({ command: e.target.value })} />
            </Field>
            {split && (
              <p className="cap-note text-sm">
                <Icon name="info" size={15} />
                <span>{tr("capabilities.il_comando_contiene_spazi_se")}</span>
                <Button size="sm" onClick={() => patch(split)}>{tr("capabilities.dividi")}</Button>
              </p>
            )}
            <Field label={tr("capabilities.argomenti")} htmlFor="mcp-args" hint="Invio aggiunge un argomento (anche incollando una riga). Le virgolette tengono insieme gli spazi: &quot;~/Mie Cartelle&quot;.">
              <TagInput id="mcp-args" label={tr("capabilities.argomenti")} value={draft.args} onChange={(args) => patch({ args })} parse={splitShellWords} placeholder={tr("capabilities.y_modelcontextprotocol_server_filesystem_mie")} />
            </Field>
            <GroupField label={tr("capabilities.variabili_d_ambiente")} error={shown.env} hint={tr("capabilities.i_valori_di_chiavi_token")}>
              <KeyValueEditor
                idPrefix="mcp-env" label={tr("capabilities.variabili_d_ambiente")} rows={draft.env} onChange={(env) => patch({ env })} keyLabel="Variabile" valueLabel="Valore"
                keyPlaceholder="GITHUB_TOKEN" isSecret={isSecretKey} mask={SECRET_MASK} addLabel={tr("capabilities.aggiungi_variabile")} emptyText={tr("capabilities.nessuna_variabile_oltre_a_quelle")}
              />
            </GroupField>
          </>
        ) : (
          <>
            <Field label={tr("capabilities.indirizzo")} htmlFor="mcp-url" error={shown.url} hint={tr("capabilities.endpoint_mcp_in_streamable_http")}>
              <Input id="mcp-url" mono value={draft.url} placeholder="http://127.0.0.1:8931/mcp" aria-invalid={!!shown.url} autoComplete="off" spellCheck={false} onChange={(e) => patch({ url: e.target.value })} />
            </Field>
            <GroupField label={tr("capabilities.header_http")} error={shown.headers} hint={tr("capabilities.authorization_e_gli_header_che")}>
              <KeyValueEditor
                idPrefix="mcp-hdr" label={tr("capabilities.header_http")} rows={draft.headers} onChange={(headers) => patch({ headers })} keyLabel="Header" valueLabel="Valore"
                keyPlaceholder="Authorization" valuePlaceholder={tr("capabilities.bearer")} isSecret={isSecretKey} mask={SECRET_MASK} addLabel={tr("capabilities.aggiungi_header")} emptyText={tr("capabilities.nessun_header_aggiuntivo")}
              />
            </GroupField>
          </>
        )}

        {draft.transport === "http" && (
          <Field label={tr("capabilities.client_id_oauth_facoltativo")} htmlFor="mcp-client-id" hint={tr("capabilities.serve_solo_se_il_server")}>
            <Input id="mcp-client-id" mono value={draft.oauthClientId} autoComplete="off" spellCheck={false} onChange={(e) => patch({ oauthClientId: e.target.value })} />
          </Field>
        )}

        <div className="row row-between nowrap">
          <div className="field">
            <span className="field-label" id="mcp-sampling-label">{tr("capabilities.consenti_al_server_di_usare")}</span>
            <span className="hint">{tr("capabilities.campionamento_mcp_il_server_puo")}</span>
          </div>
          <Toggle id="mcp-sampling" checked={!!draft.sampling} onChange={(sampling) => patch({ sampling })} label={tr("capabilities.consenti_al_server_di_usare")} />
        </div>
        {draft.sampling && (
          <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> {" "}{tr("capabilities.un_server_che_non_controlli")}</p>
        )}

        <div className="row row-between nowrap">
          <div className="field">
            <span className="field-label" id="mcp-enabled-label">{tr("capabilities.collega_il_server")}</span>
            <span className="hint">{tr("capabilities.un_server_disattivato_resta_configurato")}</span>
          </div>
          <Toggle id="mcp-enabled" checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label={tr("capabilities.collega_il_server")} />
        </div>
      </form>
    </Drawer>
  );
}
