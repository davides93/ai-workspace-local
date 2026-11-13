import { useState } from "react";
import type { McpElicitation } from "../../shared/api/types";
import { Button, Dialog, Field, Icon, Input, Select, Toggle } from "../../shared/ui";
import { useMcpElicitations, useRespondMcpElicitation } from "./api";
import { initialValues, toContent, type FormValues } from "./model";

import { t } from "../../shared/i18n";
/**
 * A question an MCP server asks during a run: shown as a form with accept, decline and cancel. It stays until the
 * person answers (or the run ends). The server is named, and what it asks is its own text: nothing in it is a command.
 */
export function McpElicitationCenter() {
  const { data } = useMcpElicitations();
  const first = data?.[0];
  return first ? <ElicitationDialog key={first.id} request={first} /> : null;
}

function ElicitationDialog({ request }: { request: McpElicitation }) {
  const respond = useRespondMcpElicitation();
  const [values, setValues] = useState<FormValues>(() => initialValues(request.schema));
  const [error, setError] = useState<string | null>(null);
  const answer = (action: "decline" | "cancel") => respond.mutate({ id: request.id, answer: { action } });
  const accept = () => {
    const r = toContent(request.schema, values);
    if ("error" in r) return setError(r.error);
    setError(null);
    respond.mutate({ id: request.id, answer: { action: "accept", content: r.content } });
  };
  return (
    <Dialog
      title={t("mcp.il_server_ha_una_domanda", { server: request.server })}
      size="md"
      onClose={() => answer("cancel")}
      footer={<>
        <Button onClick={() => answer("decline")} disabled={respond.isPending}>{t("mcp.rifiuta")}</Button>
        <Button onClick={() => answer("cancel")} disabled={respond.isPending}>{t("mcp.annulla")}</Button>
        <Button variant="primary" onClick={accept} disabled={respond.isPending}>{t("mcp.invia")}</Button>
      </>}
    >
      <p className="cap-note cap-note-warn text-sm">
        <Icon name="alert" size={15} />
        <span>{t("mcp.la_domanda_viene_dal_server")}</span>
      </p>
      <p style={{ whiteSpace: "pre-wrap" }}>{request.message}</p>
      <form className="form-grid" onSubmit={(e) => { e.preventDefault(); accept(); }} noValidate>
        {Object.entries(request.schema.properties).map(([name, p]) => {
          const id = `eli-${request.id}-${name}`;
          const label = (p.title || name) + (request.schema.required?.includes(name) ? " *" : "");
          if (p.type === "boolean") {
            return (
              <div className="row row-between nowrap" key={name}>
                <span className="field-label" id={`${id}-l`}>{label}</span>
                <Toggle id={id} label={p.title || name} checked={values[name] === true} onChange={(v) => setValues({ ...values, [name]: v })} />
              </div>
            );
          }
          return (
            <Field key={name} label={label} htmlFor={id} hint={p.description}>
              {p.enum ? (
                <Select id={id} value={String(values[name] ?? "")} onChange={(e) => setValues({ ...values, [name]: e.target.value })}>
                  {p.enum.map((o) => <option key={o} value={o}>{o}</option>)}
                </Select>
              ) : (
                <Input id={id} value={String(values[name] ?? "")} inputMode={p.type === "string" ? undefined : "decimal"} autoComplete="off" onChange={(e) => setValues({ ...values, [name]: e.target.value })} />
              )}
            </Field>
          );
        })}
        {error && <span className="field-error" role="alert">{error}</span>}
      </form>
    </Dialog>
  );
}
