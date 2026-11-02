import { Link } from "../../app/router";
import { formatMb, relTime } from "../../shared/lib/format";
import { RISK_LABEL, RISK_TONE } from "../../shared/lib/risk";
import { useSystem } from "../../shared/query/system";
import { Button, Chip, Chips, EmptyState, Icon, KeyValue, Led, Meter, Page, PageHeader, Panel, Pill, SkeletonLines, StatusPill } from "../../shared/ui";
import { useAgents, useProfiles } from "../agents/api";
import { useApprovals, useRespondApproval } from "../approvals/api";
import { useSessions } from "../chat/api";
import { isActiveDownload, useDownloads, useModels } from "../models/api";
import { useRuntimes } from "../runtimes/api";
import { useConfig } from "../settings/api";
import { FirstRun } from "./components/FirstRun";
import { checklist, defaultProfile, needsFirstRun } from "./model/overview";

export function HomeView() {
  const system = useSystem();
  const runtimes = useRuntimes();
  const models = useModels();
  const agents = useAgents();
  const profiles = useProfiles();
  const sessions = useSessions();
  const approvals = useApprovals();
  const downloads = useDownloads();
  const config = useConfig();
  const respond = useRespondApproval();

  const d = system.data?.device;
  const rts = runtimes.data ?? [];
  const running = rts.filter((r) => r.status === "running");
  const profile = defaultProfile(agents.data ?? [], profiles.data ?? [], config.data?.effective["agent.default"]);
  const profileRuntime = rts.find((r) => r.id === profile?.runtime_id);
  const pending = approvals.data ?? [];
  const recent = [...(sessions.data ?? [])].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, 5);
  const active = (downloads.data ?? []).filter(isActiveDownload);
  const ready = !!(runtimes.data && models.data && profiles.data && sessions.data);
  const showFirstRun = ready && needsFirstRun(models.data!, rts);
  const usedPct = d ? 1 - d.available_memory_mb / d.memory_mb : 0;

  return (
    <Page>
      <PageHeader eyebrow="Panoramica" title="Il tuo workspace" subtitle="Dispositivo, runtime, modelli e lavoro in corso, a colpo d'occhio." />

      {showFirstRun && <FirstRun steps={checklist({ runtimes: rts, models: models.data!, profiles: profiles.data!, sessions: sessions.data!.length })} />}

      <div className="home-grid">
        <Panel title="Questo dispositivo" icon="device" actions={d && <Pill tone="ok"><Led tone="ok" small /> {d.tier.toUpperCase()}</Pill>}>
          {!d ? <SkeletonLines lines={3} /> : (
            <div className="stack">
              <KeyValue items={[["Nome", <b>{d.name}</b>], ["Sistema", `${d.os_version} · ${d.arch}`], ["CPU", `${d.cpu.brand} · ${d.cpu.threads} thread`]]} />
              <div className="stack stack-sm">
                <div className="row row-between">
                  <span className="text-sm">Memoria</span>
                  <span className="mono faint num text-xs">{formatMb(d.memory_mb - d.available_memory_mb)} / {formatMb(d.memory_mb)}</span>
                </div>
                <Meter label="Memoria in uso" value={d.memory_mb - d.available_memory_mb} max={d.memory_mb} tone={usedPct > 0.85 ? "crit" : usedPct > 0.7 ? "warn" : "ok"} />
              </div>
              <Chips label="Acceleratori">
                {d.accelerators.map((a, i) => <Chip key={i} hot={a.kind !== "cpu"} title={a.name}>{a.kind}{a.memory_mb ? ` ${formatMb(a.memory_mb)}` : ""}</Chip>)}
              </Chips>
              <Link to="/devices" className="link-more">Dettagli del dispositivo <Icon name="forward" size={14} /></Link>
            </div>
          )}
        </Panel>

        <Panel title="Runtime attivi" icon="chip" actions={<Pill tone={running.length ? "ok" : "neutral"}>{running.length}</Pill>}>
          {running.length === 0 ? (
            <EmptyState icon="chip" title="Nessun runtime in esecuzione" description="Avviane uno per far lavorare gli agenti." action={<Link to="/runtimes" className="btn sm">Apri i runtime</Link>} />
          ) : (
            <div className="list list-flush">
              {running.map((r) => (
                <Link key={r.id} to={`/runtimes/${encodeURIComponent(r.id)}`} className="list-item clickable">
                  <Icon name="chip" />
                  <div className="grow"><b>{r.name}</b><div className="faint mono ellipsis text-xs">{r.status_detail ?? r.endpoint}</div></div>
                  <StatusPill status={r.status} />
                </Link>
              ))}
            </div>
          )}
        </Panel>

        <Panel title="Modello predefinito" icon="cube">
          {!profile ? (
            <EmptyState icon="cube" title="Nessun profilo" description="Crea un profilo di inferenza per scegliere runtime e modello." action={<Link to="/agents" className="btn sm">Apri i profili</Link>} />
          ) : (
            <div className="stack">
              <KeyValue items={[
                ["Profilo", <b>{profile.name}</b>],
                ["Modello", <span className="mono">{profile.model}</span>],
                ["Runtime", <span className="row"><span className="mono">{profile.runtime_id}</span>{profileRuntime && <StatusPill status={profileRuntime.status} />}</span>],
              ]} />
              <Link to="/models" className="link-more">Gestisci i modelli <Icon name="forward" size={14} /></Link>
            </div>
          )}
        </Panel>

        <Panel title="Conferme in attesa" icon="shield" actions={<Pill tone={pending.length ? "warn" : "neutral"}>{pending.length}</Pill>}>
          {pending.length === 0 ? (
            <p className="muted text-sm">Nessuna conferma richiesta: gli agenti non sono in attesa di te.</p>
          ) : (
            <div className="stack">
              {pending.slice(0, 3).map((a) => (
                <div key={a.id} className="approval approval-compact">
                  <div className="row"><span className="mono text-xs">{a.tool}</span><Pill tone={RISK_TONE[a.risk]}>rischio {RISK_LABEL[a.risk]}</Pill></div>
                  <span className="text-sm">{a.reason}</span>
                  <div className="row">
                    <Button size="sm" variant="primary" onClick={() => respond.mutate({ request: a, decision: "allow" })} disabled={respond.isPending}>Consenti</Button>
                    <Button size="sm" variant="danger" onClick={() => respond.mutate({ request: a, decision: "deny" })} disabled={respond.isPending}>Nega</Button>
                  </div>
                </div>
              ))}
              {pending.length > 3 && <span className="faint text-sm">e altre {pending.length - 3}</span>}
            </div>
          )}
        </Panel>

        <Panel title="Sessioni recenti" icon="chat" actions={<Link to="/chat" className="btn sm ghost">Apri il Workspace</Link>}>
          {recent.length === 0 ? (
            <EmptyState icon="chat" title="Ancora nessuna sessione" action={<Link to="/chat" className="btn sm primary">Inizia una conversazione</Link>} />
          ) : (
            <div className="list list-flush">
              {recent.map((s) => (
                <Link key={s.id} to={`/chat/${encodeURIComponent(s.id)}`} className="list-item clickable">
                  <Led tone={s.last_status === "failed" ? "crit" : s.last_status === "running" ? "run" : s.last_status ? "ok" : "off"} small />
                  <div className="grow"><b className="ellipsis block">{s.title}</b></div>
                  <span className="faint text-xs">{relTime(s.updated_at)}</span>
                </Link>
              ))}
            </div>
          )}
        </Panel>

        <Panel title="Download in corso" icon="download" actions={<Link to="/models/downloads" className="btn sm ghost">Tutti</Link>}>
          {active.length === 0 ? (
            <p className="muted text-sm">Nessun download attivo.</p>
          ) : (
            <div className="stack">
              {active.map((j) => (
                <div key={j.id} className="stack stack-sm">
                  <div className="row row-between"><span className="mono ellipsis text-sm">{j.model_ref || j.model_id}</span><StatusPill status={j.status} /></div>
                  <Meter label={`Avanzamento di ${j.model_ref || j.model_id}`} value={j.received} max={j.total} />
                </div>
              ))}
            </div>
          )}
        </Panel>
      </div>
    </Page>
  );
}
