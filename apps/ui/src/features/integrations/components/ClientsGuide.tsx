import { useState } from "react";
import { Link } from "../../../app/router";
import { CodeBlock, Icon, Panel, TabPanel, Tabs } from "../../../shared/ui";
import { buildClientGuides } from "../model/guide";

import { t, tNode } from "../../../shared/i18n";
/**
 * How to point other programs at AI Workspace: models on `/v1`, agents on `/agui/{agent}` — which token, what the
 * browser may do, which network, which limits — and the configuration of the two CopilotKit projects that can use it.
 */
export function ClientsGuide({ base, token }: {
  /** Base URL of the examples (this computer, or its LAN address). */
  base: string;
  /** The client is on another device: the examples carry the token. */
  token: boolean;
}) {
  const guides = buildClientGuides(base, { token });
  const [tab, setTab] = useState<string>(guides[0].id);
  const active = guides.find((g) => g.id === tab) ?? guides[0];
  return (
    <Panel icon="plug" title={t("integrations.guida_titolo")}>
      <div className="stack">
        <p className="muted text-sm">
          {tNode("integrations.guida_intro", { code: <code>/v1</code>, code2: <code>/agui/&#123;agente&#125;</code> })}
        </p>
        <div className="int-notes">
          <section className="int-note" aria-label={t("integrations.guida_token_titolo")}>
            <h3><Icon name="key" size={16} /> {t("integrations.guida_token_titolo")}</h3>
            <p className="muted text-sm">
              {tNode("integrations.guida_token_testo", { code: <code>/v1</code>, code2: <code>/agui</code>, link: <Link to="/security">{t("integrations.sicurezza")}</Link> })}
            </p>
          </section>
          <section className="int-note" aria-label={t("integrations.guida_cors_titolo")}>
            <h3><Icon name="globe" size={16} /> {t("integrations.guida_cors_titolo")}</h3>
            <p className="muted text-sm">
              {tNode("integrations.guida_cors_testo", { code: <code>/v1</code>, code2: <code>/agui</code> })}
            </p>
          </section>
          <section className="int-note" aria-label={t("integrations.guida_rete_titolo")}>
            <h3><Icon name="link" size={16} /> {t("integrations.guida_rete_titolo")}</h3>
            <p className="muted text-sm">
              {tNode("integrations.guida_rete_testo", { code: <code>aiw serve --lan</code>, code2: <code>127.0.0.1</code> })}
            </p>
          </section>
          <section className="int-note" aria-label={t("integrations.guida_limiti_titolo")}>
            <h3><Icon name="shield" size={16} /> {t("integrations.guida_limiti_titolo")}</h3>
            <p className="muted text-sm">{t("integrations.guida_limiti_testo")}</p>
          </section>
        </div>
        <h3 className="int-subtitle">{t("integrations.guida_terzi_titolo")}</h3>
        <p className="muted text-sm">
          {tNode("integrations.guida_terzi_testo", { code: <code>tests/agui-client</code>, code2: <code>@ag-ui/client</code> })}
        </p>
        <Tabs
          idPrefix="snip-guide" label={t("integrations.guida_terzi_titolo")} value={active.id} onChange={setTab}
          tabs={guides.map((g) => ({ id: g.id, label: g.title }))}
        />
        <TabPanel idPrefix="snip-guide" id={active.id}>
          <div className="stack">
            <p className="muted text-sm">{active.note}</p>
            <CodeBlock title={active.title} code={active.code} />
          </div>
        </TabPanel>
      </div>
    </Panel>
  );
}
