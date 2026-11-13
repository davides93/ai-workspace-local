import { useState } from "react";
import { useApi } from "../../shared/api/context";
import type { McpPromptInfo } from "../../shared/api/types";
import { Button, Dialog, Field, Input, Select } from "../../shared/ui";

import { t, t as tr } from "../../shared/i18n";
/**
 * «Prompt MCP» (developer mode): pick a prompt of a connected server, fill its arguments, and the text it renders
 * goes into the message box. Nothing is sent: the person reads and edits it first.
 */
export function McpPromptPicker({ prompts, onInsert, onClose }: {
  prompts: McpPromptInfo[];
  onInsert: (text: string) => void;
  onClose: () => void;
}) {
  const api = useApi();
  const [index, setIndex] = useState(0);
  const [args, setArgs] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const p = prompts[index];

  const insert = async () => {
    const missing = p.arguments.find((a) => a.required && !(args[a.name] ?? "").trim());
    if (missing) return setError(tr("mcp.compila", { name: missing.name }));
    setBusy(true);
    setError(null);
    try {
      const filled = Object.fromEntries(Object.entries(args).filter(([k, v]) => v.trim() !== "" && p.arguments.some((a) => a.name === k)));
      const r = await api.mcpPromptGet(p.server_id, p.name, filled);
      onInsert(r.text);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : tr("mcp.il_server_non_ha_dato"));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={t("mcp.prompt_mcp")} size="md" onClose={onClose}
      footer={<>
        <Button onClick={onClose}>{t("mcp.annulla")}</Button>
        <Button variant="primary" disabled={busy} onClick={() => void insert()}>{busy ? t("mcp.carico") : t("mcp.inserisci_nel_messaggio")}</Button>
      </>}
    >
      <form className="form-grid" onSubmit={(e) => { e.preventDefault(); void insert(); }} noValidate>
        <Field label={t("mcp.prompt")} htmlFor="mcp-prompt" hint={p.description || t("mcp.il_testo_viene_inserito_nel")}>
          <Select id="mcp-prompt" value={index} onChange={(e) => { setIndex(Number(e.target.value)); setArgs({}); setError(null); }}>
            {prompts.map((x, i) => <option key={`${x.server_id}:${x.name}`} value={i}>{x.server} · {x.name}</option>)}
          </Select>
        </Field>
        {p.arguments.map((a) => (
          <Field key={a.name} label={a.name + (a.required ? " *" : "")} htmlFor={`mcp-arg-${a.name}`} hint={a.description || undefined}>
            <Input id={`mcp-arg-${a.name}`} value={args[a.name] ?? ""} autoComplete="off" onChange={(e) => setArgs({ ...args, [a.name]: e.target.value })} />
          </Field>
        ))}
        {error && <span className="field-error" role="alert">{error}</span>}
      </form>
    </Dialog>
  );
}
