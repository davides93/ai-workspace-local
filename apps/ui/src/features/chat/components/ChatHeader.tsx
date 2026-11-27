import { Link } from "../../../app/router";
import type { ReactNode } from "react";
import { SIMPLE_AGENT_ID } from "../../../shared/lib/agents";
import { Button, Icon, IconButton, Led, Pill, type PillTone } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, Session } from "../../../shared/api/types";
import { modeInfo, RUN_STATUS_LABEL, sandboxInfo } from "../model/labels";

import { t, t as tr } from "../../../shared/i18n";
/** Id of the «Browser» button: the chat gives the focus back to it when the panel closes. */
export const BROWSER_BUTTON_ID = "ws-browser-btn";
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
 * The simple interface keeps the title, the state, the model (as text: the profiles page is not there) and the
 * approval mode; the agent only when it is not the Assistente, and neither sandbox nor worktree. On a phone it
 * has a «Menu» button, since there is no tab bar.
 */
export function ChatHeader({ session, agent, profile, role, running, simple = false, sessionsButton, inspectorOpen, onToggleInspector, onOpenWorktree, onOpenBrowser, browserLive, onOpenMenu }: {
  session: Session | null;
  agent: AgentProfile | null;
  profile: InferenceProfile | null;
  /** Simple interface: the role of the model («Intelligente»), shown before the model's own name; absent when unknown. */
  role?: string;
  running: boolean;
  /** The simple interface: fewer details, nothing that leads to a hidden section. */
  simple?: boolean;
  /** Present when the session list is off-canvas. */
  sessionsButton?: { onClick: () => void; expanded: boolean };
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  onOpenWorktree: () => void;
  /** Present on devices whose agent browser is a panel that can be closed (iPhone, iPad, Android). */
  onOpenBrowser?: () => void;
  /**
   * The «Browser» button of the live view (a browser runs, or the agent asks for help): opens and closes the panel. `status` is read after the name;
   * `attention`: the agent is waiting for the person to step in; `overlay`: the panel opens as a dialog.
   */
  browserLive?: { open: boolean; onToggle: () => void; status: string; attention: boolean; overlay: boolean };
  /** Simple interface: opens the menu (shown on phones only, where there is no rail). */
  onOpenMenu?: () => void;
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
        {browserLive && (
          <Button
            id={BROWSER_BUTTON_ID} size="sm" icon="globe" className={browserLive.open ? "ws-browser-btn is-on" : "ws-browser-btn"} title={browserLive.open ? t("browser.chiudi_vista") : t("browser.apri_vista")}
            aria-label={`${t("browser.pulsante")}: ${browserLive.status}`} aria-expanded={browserLive.open} aria-haspopup={browserLive.overlay ? "dialog" : undefined} onClick={browserLive.onToggle}
          >
            <span className="ws-browser-label">{t("browser.pulsante")}</span>
            {browserLive.attention && <Led tone="warn" small />}
          </Button>
        )}
        {onOpenBrowser && <IconButton aria-label={t("chat.browser_dell_agente")} title={t("chat.apri_il_browser_dell_agente")} icon="globe" onClick={onOpenBrowser} />}
        <IconButton aria-label={t("chat.pannello_del_run")} aria-pressed={inspectorOpen} icon="panel" onClick={onToggleInspector} className={inspectorOpen ? "is-on" : undefined} />
        {onOpenMenu && <IconButton aria-label={t("shell.menu")} icon="more" aria-haspopup="dialog" className="ws-menu-btn" onClick={onOpenMenu} />}
      </div>
      {session && agent && (
        <ul className="ws-head-meta" aria-label={t("chat.come_lavora_questa_sessione")}>
          {(!simple || agent.id !== SIMPLE_AGENT_ID) && <Meta icon="agent" label={t("chat.agente")} title={agent.description || agent.name}>{agent.name}</Meta>}
          {profile && (
            <Meta icon="cube" label={t("chat.modello")} title={t("chat.profilo", { name: profile.name, runtime_id: profile.runtime_id, p: profile.model || "nessun modello" })}>
              {simple
                ? <span className="ws-meta-text">{role ?? profile.name}</span>
                : <Link className="ws-meta-text" to="/models/profiles" title={t("chat.gestisci_i_profili_di_inferenza")}>{profile.name}</Link>}
              {profile.model && <span className="mono ws-meta-model">{profile.model}</span>}
            </Meta>
          )}
          <Meta icon="shield" label={t("chat.approvazione")} tone={mode.tone} title={`${mode.label}: ${mode.hint}`}>{mode.label}</Meta>
          {!simple && <Meta icon="lock" label={t("chat.sandbox")} tone={sandbox.tone} title={`${sandbox.label}: ${sandbox.hint}`}>{sandbox.label}</Meta>}
          {!simple && session.worktree && (
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
