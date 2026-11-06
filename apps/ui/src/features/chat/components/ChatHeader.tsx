import { Link } from "../../../app/router";
import type { ReactNode } from "react";
import { Icon, IconButton, Led, Pill, type PillTone } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, Session } from "../../../shared/api/types";
import { modeInfo, RUN_STATUS_LABEL, sandboxInfo } from "../model/labels";

/** One fact about the session: icon, text, a tooltip and a spoken label so the icon is not the only cue. */
function Meta({ icon, label, tone = "neutral", title, children }: { icon: string; label: string; tone?: PillTone; title?: string; children: ReactNode }) {
  return (
    <li>
      <Pill tone={tone} title={title ?? label}>
        <Icon name={icon} size={13} />
        <span className="sr-only">{label}: </span>
        {children}
      </Pill>
    </li>
  );
}

/**
 * Header of the conversation: title and controls, then who works on it (agent, profile and model,
 * approval mode, sandbox, worktree) so the safety setup is never out of sight.
 */
export function ChatHeader({ session, agent, profile, running, sessionsButton, inspectorOpen, onToggleInspector, onOpenWorktree }: {
  session: Session | null;
  agent: AgentProfile | null;
  profile: InferenceProfile | null;
  running: boolean;
  /** Present when the session list is off-canvas. */
  sessionsButton?: { onClick: () => void; expanded: boolean };
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  onOpenWorktree: () => void;
}) {
  const mode = modeInfo(agent?.mode);
  const sandbox = sandboxInfo(agent?.sandbox);
  return (
    <header className="ws-head">
      <div className="ws-head-top">
        {sessionsButton && <IconButton aria-label="Sessioni" icon="list" aria-haspopup="dialog" aria-expanded={sessionsButton.expanded} onClick={sessionsButton.onClick} />}
        <h2 className="ws-title" title={session?.title}>{session?.title ?? "Workspace"}</h2>
        {running && <Pill tone="run"><Led tone="run" small />In esecuzione</Pill>}
        {!running && session?.last_status && session.last_status !== "succeeded" && (
          <Pill tone={session.last_status === "failed" ? "crit" : "neutral"} title="Esito dell'ultimo run">{RUN_STATUS_LABEL[session.last_status] ?? session.last_status}</Pill>
        )}
        <IconButton aria-label="Pannello del run" aria-pressed={inspectorOpen} icon="panel" onClick={onToggleInspector} className={inspectorOpen ? "is-on" : undefined} />
      </div>
      {session && agent && (
        <ul className="ws-head-meta" aria-label="Come lavora questa sessione">
          <Meta icon="agent" label="Agente" title={agent.description || agent.name}>{agent.name}</Meta>
          {profile && (
            <Meta icon="cube" label="Modello" title={`Profilo ${profile.name} · ${profile.runtime_id} · ${profile.model || "nessun modello"}`}>
              <Link className="ws-meta-text" to="/models/profiles" title="Gestisci i profili di inferenza">{profile.name}</Link>
              {profile.model && <span className="mono ws-meta-model">{profile.model}</span>}
            </Meta>
          )}
          <Meta icon="shield" label="Approvazione" tone={mode.tone} title={`${mode.label}: ${mode.hint}`}>{mode.label}</Meta>
          <Meta icon="lock" label="Sandbox" tone={sandbox.tone} title={`${sandbox.label}: ${sandbox.hint}`}>{sandbox.label}</Meta>
          {session.worktree && (
            <li>
              <button type="button" className="pill neutral mono pill-btn" onClick={onOpenWorktree} title={`Worktree isolato: ${session.worktree.path}`}>
                <Icon name="branch" size={13} />
                <span className="sr-only">Worktree: </span>
                {session.worktree.branch}
              </button>
            </li>
          )}
        </ul>
      )}
    </header>
  );
}
