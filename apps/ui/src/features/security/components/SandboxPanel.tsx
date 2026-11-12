import { Link } from "../../../app/router";
import { Button, Icon, KeyValue, Panel, Pill, QueryState, Section, Segmented, TagInput } from "../../../shared/ui";
import { approvalMode, sandboxMode, SANDBOX_MODES } from "../../../shared/lib/policyModes";
import { useSystem } from "../../../shared/query/system";
import { useConfig, useSetConfig } from "../../settings/api";
import { CONTAINMENT, containmentFor, KNOWN_LIMITS } from "../model/sandbox";

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

      <QueryState query={cfg}>
        {(view) => {
          const net = view.effective["sandbox.shell_network"] === "on" ? "on" : "off";
          const list = (k: string) => (Array.isArray(view.effective[k]) ? (view.effective[k] as unknown[]).map(String) : []);
          return (
            <>
              {view.effective["sandbox.shell_network_notice"] === true && (
                <div className="notice warn" role="status">
                  <b>Novità: la rete dei comandi ora si governa da qui.</b> Per non cambiare il tuo modo di lavorare, in questa installazione è rimasta <b>accesa</b> (com'era prima).
                  Le nuove installazioni partono con la rete spenta: ti consigliamo di spegnerla se non ti serve per installare pacchetti. Da ora la cartella dei dati dell'app e le cartelle delle credenziali sono nascoste ai comandi, e le variabili d'ambiente con chiavi e token non arrivano più ai comandi.
                  <div className="mt-2"><Button size="sm" onClick={() => save("sandbox.shell_network_notice", false, "Avviso sulla rete dei comandi")}>Ho capito</Button></div>
                </div>
              )}
              <Section title="Rete e credenziali dei comandi" hint="Valgono per i comandi di shell e per i server MCP isolati. Sono impostazioni della macchina: un repository non può allentarle.">
                <Panel>
                  <div className="stack">
                    <div>
                      <b>Rete dei comandi</b>
                      <Segmented
                        label="Rete dei comandi" value={net}
                        options={[{ id: "off", label: "Spenta" }, { id: "on", label: "Accesa" }]}
                        onChange={(v) => save("sandbox.shell_network", v, "Rete dei comandi")}
                      />
                      <p className="faint text-sm">
                        {windows
                          ? "Su Windows la rete dei comandi non si può chiudere: questo interruttore non ha effetto sui comandi di shell (resta valido per i tool che lo leggono). Vedi i limiti sotto."
                          : !shellContained
                            ? "Su questo dispositivo i comandi non sono contenuti (manca bubblewrap o non c'è una sandbox): l'interruttore non ha effetto finché non c'è."
                            : net === "off"
                              ? "Spenta: i comandi non raggiungono la rete. Se un comando fallisce per questo, il risultato lo dice. Per installare pacchetti o scaricare file accendila."
                              : "Accesa: i comandi possono usare la rete. Un agente può comunque chiuderla per una singola chiamata, mai aprirla."}
                      </p>
                    </div>
                    <div>
                      <b>Cartelle nascoste ai comandi</b>
                      <TagInput
                        id="sandbox-deny-read" label="Cartelle nascoste" value={list("sandbox.deny_read")}
                        onChange={(v) => save("sandbox.deny_read", v, "Cartelle nascoste")} placeholder="~/.ssh"
                      />
                      <p className="faint text-sm">
                        Oltre alla cartella dei dati dell'app (sempre nascosta) i comandi non vedono queste cartelle: chiavi SSH, credenziali cloud, token di registri. Il compromesso: anche <code>git</code> via SSH non trova la tua chiave.
                        Per consentirlo togli <code>~/.ssh</code> dall'elenco (o usa HTTPS con un token). I tool di file degli agenti rispettano lo stesso elenco.
                      </p>
                    </div>
                    <div>
                      <b>Variabili d'ambiente consentite</b>
                      <TagInput
                        id="sandbox-env-allow" label="Variabili consentite" value={list("sandbox.env_allow")}
                        onChange={(v) => save("sandbox.env_allow", v, "Variabili consentite")} placeholder="NPM_TOKEN"
                      />
                      <p className="faint text-sm">
                        Ai comandi non arrivano le variabili che sembrano segreti (<code>*_TOKEN</code>, <code>*_KEY</code>, <code>*_SECRET</code>, <code>*PASSWORD*</code>, <code>AIW_*</code>, credenziali cloud). Se un comando ne ha davvero bisogno, scrivi qui il nome esatto.
                      </p>
                    </div>
                  </div>
                </Panel>
              </Section>
            </>
          );
        }}
      </QueryState>

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
            <thead><tr><th scope="col">Sistema</th><th scope="col">Meccanismo</th><th scope="col">Scritture</th><th scope="col">Rete</th><th scope="col">Cartelle nascoste</th><th scope="col">Server MCP</th><th scope="col">Nota</th></tr></thead>
            <tbody>
              {CONTAINMENT.map((c) => (
                <tr key={c.os} aria-current={c.os === here?.os ? "true" : undefined} className={c.os === here?.os ? "current-os" : undefined}>
                  <th scope="row" className="cell-os">{c.label}{c.os === here?.os && <Pill tone="run">questo dispositivo</Pill>}</th>
                  <td>{c.mechanism ?? <span className="faint">nessuno</span>}</td>
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
