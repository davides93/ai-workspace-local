import { Link } from "../../../app/router";
import { Button, Icon, KeyValue, Panel, Pill, QueryState, Section, Segmented, TagInput } from "../../../shared/ui";
import { approvalMode, sandboxMode, SANDBOX_MODES } from "../../../shared/lib/policyModes";
import { useSystem } from "../../../shared/query/system";
import { useConfig, useSetConfig } from "../../settings/api";
import { CONTAINMENT, containmentFor, KNOWN_LIMITS } from "../model/sandbox";

import { t, tNode, t as tr } from "../../../shared/i18n";
const asText = (v: unknown, fallback: string) => (typeof v === "string" && v ? v : fallback);

/** Effective configuration and what actually contains a command on this operating system. */
export function SandboxPanel() {
  const cfg = useConfig();
  const system = useSystem();
  const setConfig = useSetConfig();
  const here = containmentFor(system.data?.device.os);
  const shellContained = !!system.data?.shell_sandbox && system.data.shell_sandbox !== "none";
  const windows = system.data?.device.os === "windows";
  const save = (key: string, value: unknown, title: string) => setConfig.mutate({ layer: "global", key, value, title });

  return (
    <div className="stack stack-lg">
      <Section title={t("security.valori_correnti")} hint={t("security.dalla_configurazione_a_strati_ogni")}>
        <QueryState query={cfg}>
          {(view) => {
            const mode = asText(view.effective["policy.mode"], "ask");
            const sandbox = asText(view.effective["sandbox.mode"], "workspace-write");
            const domains = Array.isArray(view.effective["network.allow_domains"]) ? (view.effective["network.allow_domains"] as unknown[]).map(String) : [];
            const m = approvalMode(mode);
            const s = sandboxMode(sandbox);
            return (
              <Panel>
                <KeyValue items={[
                  [tr("security.modalita_di_approvazione"), <><Pill tone={m?.tone ?? "neutral"}>{m?.label ?? mode}</Pill> <span className="muted">{m?.summary}</span></>],
                  [tr("security.sandbox"), <><Pill tone={s?.tone ?? "neutral"}>{s?.label ?? sandbox}</Pill> <span className="muted">{s?.summary}</span></>],
                  [tr("security.domini_consentiti_a_web_e"), domains.length ? <span className="mono">{domains.join(", ")}</span> : <span className="muted">{t("security.tutti_i_domini_pubblici")}</span>],
                  system.data && [tr("security.questo_dispositivo_2"), <>{system.data.device.name} · {here?.label ?? system.data.device.os}</>],
                ]} />
                <p className="faint text-sm mt-3">
                  {tNode("security.le_predefinite_valgono_per_i", { link: <Link to="/settings">{t("security.impostazioni")}</Link>, link2: <Link to="/agents">{t("security.agenti")}</Link> })}</p>
              </Panel>
            );
          }}
        </QueryState>
      </Section>

      <QueryState query={cfg}>
        {(view) => {
          const net = view.effective["sandbox.shell_network"] === "on" ? "on" : "off";
          const list = (k: string) => (Array.isArray(view.effective[k]) ? (view.effective[k] as unknown[]).map(String) : []);
          return (
            <>
              {view.effective["sandbox.shell_network_notice"] === true && (
                <div className="notice warn" role="status">
                  <b>{t("security.novita_la_rete_dei_comandi")}</b> {tNode("security.shell_network_notice_body", { on: <b>{t("security.accesa")}</b> })}
                  <div className="mt-2"><Button size="sm" onClick={() => save("sandbox.shell_network_notice", false, t("security.shell_network_notice_title"))}>{t("security.ho_capito")}</Button></div>
                </div>
              )}
              <Section title={t("security.rete_e_credenziali_dei_comandi")} hint={t("security.valgono_per_i_comandi_di")}>
                <Panel>
                  <div className="stack">
                    <div>
                      <b>{t("security.rete_dei_comandi")}</b>
                      <Segmented
                        label={t("security.rete_dei_comandi")} value={net}
                        options={[{ id: "off", label: t("security.spenta") }, { id: "on", label: t("security.accesa_2") }]}
                        onChange={(v) => save("sandbox.shell_network", v, tr("security.rete_dei_comandi"))}
                      />
                      <p className="faint text-sm">
                        {windows ? t("security.su_windows_la_rete_dei") : !shellContained ? t("security.su_questo_dispositivo_i_comandi") : net === "off" ? t("security.spenta_i_comandi_non_raggiungono") : t("security.accesa_i_comandi_possono_usare")}
                      </p>
                    </div>
                    <div>
                      <b>{t("security.cartelle_nascoste_ai_comandi")}</b>
                      <TagInput
                        id="sandbox-deny-read" label={t("security.cartelle_nascoste")} value={list("sandbox.deny_read")}
                        onChange={(v) => save("sandbox.deny_read", v, tr("security.cartelle_nascoste"))} placeholder="~/.ssh"
                      />
                      <p className="faint text-sm">
                        {tNode("security.oltre_alla_cartella_dei_dati", { code: <code>{t("security.git")}</code>, code2: <code>~/.ssh</code> })}</p>
                    </div>
                    <div>
                      <b>{t("security.variabili_d_ambiente_consentite")}</b>
                      <TagInput
                        id="sandbox-env-allow" label={t("security.variabili_consentite")} value={list("sandbox.env_allow")}
                        onChange={(v) => save("sandbox.env_allow", v, tr("security.variabili_consentite"))} placeholder="NPM_TOKEN"
                      />
                      <p className="faint text-sm">
                        {tNode("security.ai_comandi_non_arrivano_le", { code: <code>*_TOKEN</code>, code2: <code>*_KEY</code>, code3: <code>*_SECRET</code>, code4: <code>{t("security.password")}</code>, code5: <code>AIW_*</code> })}</p>
                    </div>
                  </div>
                </Panel>
              </Section>
            </>
          );
        }}
      </QueryState>

      <Section title={t("security.modalita_della_sandbox")} hint={t("security.quanto_lontano_arrivano_i_comandi")}>
        <div className="mode-cards">
          {SANDBOX_MODES.map((m) => (
            <div key={m.id} className="mode-card">
              <Pill tone={m.tone}>{m.label}</Pill>
              <code>{m.id}</code>
              <p className="text-sm muted">{m.summary}</p>
            </div>
          ))}
        </div>
      </Section>

      <Section title={t("security.cosa_contiene_un_comando_per")} hint={t("security.cosa_fa_davvero_il_backend")}>
        <div className="table-wrap" tabIndex={0} role="region" aria-label={t("security.contenimento_dei_comandi_per_sistema")}>
          <table className="data" aria-label={t("security.contenimento_dei_comandi_per_sistema")}>
            <thead><tr><th scope="col">{t("security.sistema")}</th><th scope="col">{t("security.meccanismo")}</th><th scope="col">{t("security.scritture")}</th><th scope="col">{t("security.rete")}</th><th scope="col">{t("security.cartelle_nascoste")}</th><th scope="col">{t("security.server_mcp")}</th><th scope="col">{t("security.nota")}</th></tr></thead>
            <tbody>
              {CONTAINMENT.map((c) => (
                <tr key={c.os} aria-current={c.os === here?.os ? "true" : undefined} className={c.os === here?.os ? "current-os" : undefined}>
                  <th scope="row" className="cell-os">{c.label}{c.os === here?.os && <Pill tone="run">{t("security.questo_dispositivo")}</Pill>}</th>
                  <td>{c.mechanism ?? <span className="faint">{t("security.nessuno")}</span>}</td>
                  <td className="cell-detail">{c.writes}</td>
                  <td className="cell-detail">{c.network}</td>
                  <td className="cell-detail">{c.reads}</td>
                  <td className="cell-detail">{c.mcp}</td>
                  <td className="cell-detail">{c.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Section>

      <Section title={t("security.limiti_noti")} hint={t("security.cosa_la_sandbox_non_fa")}>
        <ul className="limits">
          {KNOWN_LIMITS.map((l) => (
            <li key={l.title} className="limit">
              <Icon name="alert" size={16} />
              <div>
                <b>{l.title}</b>
                <p className="text-sm muted">{l.body}</p>
              </div>
            </li>
          ))}
        </ul>
      </Section>
    </div>
  );
}
