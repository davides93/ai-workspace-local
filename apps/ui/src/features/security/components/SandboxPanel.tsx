import { Link } from "../../../app/router";
import { Icon, KeyValue, Panel, Pill, QueryState, Section } from "../../../shared/ui";
import { approvalMode, sandboxMode, SANDBOX_MODES } from "../../../shared/lib/policyModes";
import { useSystem } from "../../../shared/query/system";
import { useConfig } from "../../settings/api";
import { CONTAINMENT, containmentFor, KNOWN_LIMITS } from "../model/sandbox";

const asText = (v: unknown, fallback: string) => (typeof v === "string" && v ? v : fallback);

/** Effective configuration and what actually contains a command on this operating system. */
export function SandboxPanel() {
  const cfg = useConfig();
  const system = useSystem();
  const here = containmentFor(system.data?.device.os);

  return (
    <div className="stack stack-lg">
      <Section title="Valori correnti" hint="Dalla configurazione a strati. Ogni agente ha la propria modalità e la propria sandbox: queste sono le predefinite.">
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
                  ["Modalità di approvazione", <><Pill tone={m?.tone ?? "neutral"}>{m?.label ?? mode}</Pill> <span className="muted">{m?.summary}</span></>],
                  ["Sandbox", <><Pill tone={s?.tone ?? "neutral"}>{s?.label ?? sandbox}</Pill> <span className="muted">{s?.summary}</span></>],
                  ["Domini consentiti a web e browser", domains.length ? <span className="mono">{domains.join(", ")}</span> : <span className="muted">Tutti i domini pubblici</span>],
                  system.data && ["Questo dispositivo", <>{system.data.device.name} · {here?.label ?? system.data.device.os}</>],
                ]} />
                <p className="faint text-sm mt-3">
                  Le predefinite valgono per i passi «tool» dei workflow e per i client esterni (MCP e API). Per cambiarle apri le <Link to="/settings">Impostazioni</Link>;
                  la modalità e la sandbox di un agente si cambiano dalla sua scheda in <Link to="/agents">Agenti</Link>.
                </p>
              </Panel>
            );
          }}
        </QueryState>
      </Section>

      <Section title="Modalità della sandbox" hint="Quanto lontano arrivano i comandi e i tool di file di un agente.">
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

      <Section title="Cosa contiene un comando, per sistema" hint="Cosa fa davvero il backend su ciascun sistema operativo.">
        <div className="table-wrap">
          <table className="data" aria-label="Contenimento dei comandi per sistema operativo">
            <thead><tr><th scope="col">Sistema</th><th scope="col">Meccanismo</th><th scope="col">Scritture</th><th scope="col">Rete</th><th scope="col">Nota</th></tr></thead>
            <tbody>
              {CONTAINMENT.map((c) => (
                <tr key={c.os} aria-current={c.os === here?.os ? "true" : undefined} className={c.os === here?.os ? "current-os" : undefined}>
                  <th scope="row" className="cell-os">{c.label}{c.os === here?.os && <Pill tone="run">questo dispositivo</Pill>}</th>
                  <td>{c.mechanism ?? <span className="faint">nessuno</span>}</td>
                  <td className="cell-detail">{c.writes}</td>
                  <td className="cell-detail">{c.network}</td>
                  <td className="cell-detail">{c.note}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Section>

      <Section title="Limiti noti" hint="Cosa la sandbox non fa oggi.">
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
