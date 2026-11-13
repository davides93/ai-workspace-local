import { Link } from "../../../app/router";
import type { ReactNode } from "react";
import { Icon, IconButton, Led, Pill, type PillTone } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, Session } from "../../../shared/api/types";
import { modeInfo, RUN_STATUS_LABEL, sandboxInfo } from "../model/labels";

import { t, t as tr } from "../../../shared/i18n";
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
export function ChatHeader({ session, agent, profile, running, sessionsButton, inspectorOpen, onToggleInspector, onOpenWorktree, onOpenBrowser }: {
  session: Session | null;
  agent: AgentProfile | null;
  profile: InferenceProfile | null;
  running: boolean;
  /** Present when the session list is off-canvas. */
  sessionsButton?: { onClick: () => void; expanded: boolean };
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  onOpenWorktree: () => void;
  /** Present on devices whose agent browser is a panel that can be closed (iPhone, iPad, Android). */
  onOpenBrowser?: () => void;
}) {
  const mode = modeInfo(agent?.mode);
  const sandbox = sandboxInfo(agent?.sandbox);
  return (
    <header className="ws-head">
      <div className="ws-head-top">
        {sessionsButton && <IconButton aria-label={t("chat.sessioni")} icon="list" aria-haspopup="dialog" aria-expanded={sessionsButton.expanded} onClick={sessionsButton.onClick} />}
        <h2 className="ws-title" title={session?.title}>{session?.title ?? t("chat.workspace")}</h2>
        {running && <Pill tone="run"><Led tone="run" small />{tr("chat.in_esecuzione")}</Pill>}
        {!running && session?.last_status && session.last_status !== "succeeded" && (
          <Pill tone={session.last_status === "failed" ? "crit" : "neutral"} title={t("chat.esito_dell_ultimo_run")}>{RUN_STATUS_LABEL[session.last_status] ?? session.last_status}</Pill>
        )}
        {onOpenBrowser && <IconButton aria-label={t("chat.browser_dell_agente")} title={t("chat.apri_il_browser_dell_agente")} icon="globe" onClick={onOpenBrowser} />}
        <IconButton aria-label={t("chat.pannello_del_run")} aria-pressed={inspectorOpen} icon="panel" onClick={onToggleInspector} className={inspectorOpen ? "is-on" : undefined} />
      </div>
      {session && agent && (
        <ul className="ws-head-meta" aria-label={t("chat.come_lavora_questa_sessione")}>
          <Meta icon="agent" label={t("chat.agente")} title={agent.description || agent.name}>{agent.name}</Meta>
          {profile && (
            <Meta icon="cube" label={t("chat.modello")} title={t("chat.profilo", { name: profile.name, runtime_id: profile.runtime_id, p: profile.model || "nessun modello" })}>
              <Link className="ws-meta-text" to="/models/profiles" title={t("chat.gestisci_i_profili_di_inferenza")}>{profile.name}</Link>
              {profile.model && <span className="mono ws-meta-model">{profile.model}</span>}
            </Meta>
          )}
          <Meta icon="shield" label={t("chat.approvazione")} tone={mode.tone} title={`${mode.label}: ${mode.hint}`}>{mode.label}</Meta>
          <Meta icon="lock" label={t("chat.sandbox")} tone={sandbox.tone} title={`${sandbox.label}: ${sandbox.hint}`}>{sandbox.label}</Meta>
          {session.worktree && (
            <li>
              <button type="button" className="pill neutral mono pill-btn" onClick={onOpenWorktree} title={t("chat.worktree_isolato", { path: session.worktree.path })}>
                <Icon name="branch" size={13} />
                <span className="sr-only">{tr("chat.worktree_3")}{" "}</span>
                {session.worktree.branch}
              </button>
            </li>
          )}
        </ul>
      )}
    </header>
  );
}
