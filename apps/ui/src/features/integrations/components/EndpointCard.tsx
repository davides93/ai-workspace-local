import { CodeBlock, CopyField, InlineText, Panel, Pill, TabPanel, Tabs } from "../../../shared/ui";
import type { Endpoint, SnippetLang } from "../model/snippets";

import { t } from "../../../shared/i18n";
const ICONS = { openai: "plug", anthropic: "plug", mcp: "tool", a2a: "link", webhook: "bolt" } as const;

/** One endpoint: its URLs to copy and the examples, one tab per language. */
export function EndpointCard({ endpoint: e, lang, onLang, needsToken }: {
  endpoint: Endpoint;
  /** Language chosen on any card: every card follows it when it has that language. */
  lang: SnippetLang;
  onLang: (l: SnippetLang) => void;
  /** The client is on another device: this endpoint wants the pairing token. */
  needsToken: boolean;
}) {
  const active = e.snippets.find((s) => s.lang === lang) ?? e.snippets[0];
  const prefix = `snip-${e.id}`;
  return (
    <Panel
      icon={ICONS[e.id]} title={e.title}
      actions={needsToken && e.guarded ? <Pill tone="warn">{t("integrations.richiede_il_token")}</Pill> : !e.guarded ? <Pill>{t("integrations.usa_il_segreto_del_workflow")}</Pill> : undefined}
    >
      <div className="stack">
        <p className="muted text-sm"><InlineText text={e.description} /></p>
        <div className="int-urls">
          <CopyField label={e.id === "webhook" ? t("integrations.url_di_base") : t("integrations.url")} value={e.url} />
          {e.extra?.map((x) => <CopyField key={x.url} label={x.label} value={x.url} />)}
        </div>
        <Tabs
          idPrefix={prefix} label={t("integrations.esempi_per", { title: e.title })} value={active.lang} onChange={onLang}
          tabs={e.snippets.map((s) => ({ id: s.lang, label: s.label }))}
        />
        <TabPanel idPrefix={prefix} id={active.lang}>
          <CodeBlock title={active.label} code={active.code} />
        </TabPanel>
      </div>
    </Panel>
  );
}
