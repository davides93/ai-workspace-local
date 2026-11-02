import { useMemo, useState } from "react";
import { useApi } from "../../shared/api/context";
import { serviceBaseUrl } from "../../shared/lib/baseUrl";
import { Page, PageHeader } from "../../shared/ui";
import { useLanAddress } from "./api";
import { AccessPanel, type Audience } from "./components/AccessPanel";
import { EndpointCard } from "./components/EndpointCard";
import { buildEndpoints, lanBaseUrl, type SnippetLang } from "./model/snippets";
import "./integrations.css";

/** `#/integrations` — the endpoints other programs use to reach models, tools and agents, with copyable examples. */
export function IntegrationsView() {
  const api = useApi();
  const lan = useLanAddress();
  const [audience, setAudience] = useState<Audience>("local");
  const [lang, setLang] = useState<SnippetLang>("curl");
  const base = audience === "lan" ? lanBaseUrl(lan.data) : serviceBaseUrl({ native: api.capabilities.native });
  const endpoints = useMemo(() => buildEndpoints(base, { token: audience === "lan" }), [base, audience]);

  return (
    <Page>
      <PageHeader
        eyebrow="Capacità" title="Integrazioni"
        subtitle="Gli endpoint con cui altri programmi usano i tuoi modelli, i tuoi tool e i tuoi agenti."
      />
      <AccessPanel audience={audience} onAudience={setAudience} lanAddress={lan.data} />
      <div className="stack stack-lg">
        {endpoints.map((e) => (
          <EndpointCard key={e.id} endpoint={e} lang={lang} onLang={setLang} needsToken={audience === "lan"} />
        ))}
      </div>
    </Page>
  );
}
