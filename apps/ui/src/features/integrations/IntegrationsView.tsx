import { useMemo } from "react";
import { Link } from "../../app/router";
import { useApi } from "../../shared/api/context";
import { serviceBaseUrl } from "../../shared/lib/baseUrl";
import { CodeBlock, CopyField, Icon, Page, PageHeader, Panel } from "../../shared/ui";
import { buildEndpoints } from "./model/snippets";

const ICONS = { openai: "plug", anthropic: "plug", mcp: "tool", a2a: "link", webhook: "bolt" } as const;

export function IntegrationsView() {
  const api = useApi();
  const base = serviceBaseUrl({ native: api.capabilities.native });
  const endpoints = useMemo(() => buildEndpoints(base), [base]);
  return (
    <Page>
      <PageHeader
        eyebrow="Capacità" title="Integrazioni"
        subtitle="Gli endpoint con cui altri programmi usano i tuoi modelli, i tuoi tool e i tuoi agenti."
      />
      <Panel className="notice">
        <div className="row nowrap">
          <Icon name="lock" size={20} />
          <div className="stack stack-sm">
            <b>Accesso e token</b>
            <span className="muted text-sm">
              Da questo computer gli endpoint rispondono senza token. I client in LAN devono inviare il token ottenuto con il pairing
              (<span className="mono">Authorization: Bearer &lt;token&gt;</span> oppure <span className="mono">x-api-key</span>), e
              il server deve essere avviato con l'accesso dalla LAN (<span className="mono">aiw serve --lan</span>). Il pairing si fa da <Link to="/devices">Dispositivi</Link>.
            </span>
          </div>
        </div>
      </Panel>
      <div className="stack stack-lg">
        {endpoints.map((e) => (
          <Panel key={e.id} icon={ICONS[e.id]} title={e.title}>
            <div className="stack">
              <p className="muted text-sm">{e.description}</p>
              <CopyField label={e.id === "webhook" ? "URL di base" : "URL"} value={e.url} />
              {e.extra?.map((x) => <CopyField key={x.url} label={x.label} value={x.url} />)}
              {e.snippets.map((s) => <CodeBlock key={s.title} title={s.title} code={s.code} />)}
            </div>
          </Panel>
        ))}
      </div>
    </Page>
  );
}
