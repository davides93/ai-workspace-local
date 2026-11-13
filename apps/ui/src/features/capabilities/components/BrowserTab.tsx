import type { ToolInfo } from "../../../shared/api/types";
import { Link } from "../../../app/router";
import { DataTable, EmptyState, Icon, Panel, Pill, QueryState, Section, type Column, type PillTone } from "../../../shared/ui";
import { useSetToolsEnabled, useTools } from "../api";
import { capabilityStatus, isBrowserTool, isComputerTool, type CapabilityStatus } from "../model/tools";
import { ToolRow } from "./ToolRow";

import { t as tr, tNode as trNode, t } from "../../../shared/i18n";
const STATUS: Record<CapabilityStatus, { label: string; tone: PillTone }> = {
  ready: { get label() { return t("capabilities.disponibile"); }, tone: "ok" },
  partial: { get label() { return t("capabilities.parziale"); }, tone: "warn" },
  off: { get label() { return t("capabilities.disattivato"); }, tone: "neutral" },
  unavailable: { get label() { return t("capabilities.non_disponibile_qui"); }, tone: "crit" },
  absent: { get label() { return t("capabilities.non_presente_in_questa_build"); }, tone: "neutral" },
};

/** Approval of each risk class per approval mode, as `aiw-agent::policy` decides it (before the policy rules). */
const MODES = ["plan", "ask", "auto-edit", "auto", "yolo"] as const;
type Cell = { label: string; tone: PillTone };
const ALLOW: Cell = { get label() { return t("capabilities.consentito"); }, tone: "ok" };
const ASK: Cell = { get label() { return t("capabilities.chiede"); }, tone: "warn" };
const DENY: Cell = { get label() { return t("capabilities.negato"); }, tone: "crit" };
const MATRIX: { risk: string; cells: Cell[] }[] = [
  { get risk() { return t("capabilities.lettura_e_rete_snapshot_screenshot"); }, cells: [ALLOW, ALLOW, ALLOW, ALLOW, ALLOW] },
  { get risk() { return t("capabilities.computer_use_clic_digitazione_tasti"); }, cells: [DENY, ASK, ASK, ASK, ALLOW] },
];

interface Capability {
  id: "browser" | "computer";
  title: string;
  icon: string;
  tools: ToolInfo[];
  how: string[];
}

function ApprovalMatrix() {
  const columns: Column<(typeof MATRIX)[number]>[] = [
    { key: "risk", header: t("capabilities.azione"), render: (r) => r.risk },
    ...MODES.map((m, i) => ({ key: m, header: <span className="mono">{m}</span>, nowrap: true, render: (r: (typeof MATRIX)[number]) => <Pill tone={r.cells[i].tone}>{r.cells[i].label}</Pill> })),
  ];
  return <DataTable label={tr("capabilities.approvazione_per_modalita")} columns={columns} rows={MATRIX} rowKey={(r) => r.risk} />;
}

/** What this build can do with the browser and the desktop, how it does it, and when it asks. */
export function BrowserTab() {
  const tools = useTools();
  const setEnabled = useSetToolsEnabled();
  return (
    <QueryState query={tools}>
      {(all) => {
        const caps: Capability[] = [
          {
            id: "browser", title: tr("capabilities.browser_controllato"), icon: "globe", tools: all.filter(isBrowserTool),
            how: [
              t("capabilities.avvia_un_profilo_dedicato_di"),
              t("capabilities.lo_snapshot_e_testo_con"),
              t("capabilities.i_siti_raggiungibili_si_limitano"),
            ],
          },
          {
            id: "computer", title: tr("capabilities.computer_use"), icon: "mouse", tools: all.filter(isComputerTool),
            how: [
              t("capabilities.cattura_lo_schermo_principale_e"),
              t("capabilities.le_coordinate_si_riferiscono_all"),
              t("capabilities.disponibile_solo_nelle_build_desktop"),
            ],
          },
        ];
        return (
          <>
            <div className="cap-grid">
              {caps.map((c) => {
                const st = capabilityStatus(c.tools);
                return (
                  <Panel key={c.id} icon={c.icon} title={<>{c.title} <Pill tone={STATUS[st.status].tone}>{STATUS[st.status].label}</Pill></>} padded={false}>
                    <div className="cap-body">
                      {st.reason && <p className="cap-note cap-note-warn text-sm"><Icon name="alert" size={15} /> {st.reason}</p>}
                      <ul className="cap-how text-sm">{c.how.map((h) => <li key={h}>{h}</li>)}</ul>
                    </div>
                    {c.tools.length === 0 ? (
                      <p className="cap-empty faint text-sm">{trNode("capabilities.no_tools_registered", { prefix: <code>{c.id}_*</code> })}</p>
                    ) : (
                      <ul className="cap-tools">
                        {c.tools.map((t) => <ToolRow key={t.name} tool={t} onToggle={(enabled) => setEnabled.mutate({ names: [t.name], enabled })} />)}
                      </ul>
                    )}
                  </Panel>
                );
              })}
            </div>

            <Section title={tr("capabilities.quando_chiedono_conferma")} hint={tr("capabilities.dipende_dalla_modalita_di_approvazione")}>
              <ApprovalMatrix />
              <p className="muted text-sm">
                {trNode("capabilities.consenti_sempre_per_la_sessione", { link: <Link to="/agents">{tr("capabilities.agenti")}</Link>, link2: <Link to="/security">{tr("capabilities.sicurezza")}</Link> })}</p>
            </Section>

            <Section title={tr("capabilities.ultime_catture")} hint={tr("capabilities.gli_screenshot_prodotti_da_browser")}>
              <EmptyState
                icon="mouse" title={tr("capabilities.le_catture_appaiono_nella_chat")}
                description={tr("capabilities.mentre_un_run_usa_browser")}
                action={<Link to="/chat" className="btn"><Icon name="chat" size={14} /> {tr("capabilities.apri_il_workspace")}</Link>}
              />
            </Section>
          </>
        );
      }}
    </QueryState>
  );
}

