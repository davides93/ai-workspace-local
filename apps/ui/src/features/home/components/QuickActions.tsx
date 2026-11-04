import type { ReactNode } from "react";
import { openPalette } from "../../../app/palette/openPalette";
import { navigate } from "../../../app/router";
import { defaultAgentId } from "../../../shared/lib/agents";
import { cx } from "../../../shared/lib/cx";
import { Icon } from "../../../shared/ui";
import { useAgents, useProfiles } from "../../agents/api";
import { useCreateSession } from "../../chat/api";
import { useRuntimeAction, useRuntimes } from "../../runtimes/api";
import { useConfig } from "../../settings/api";
import { defaultProfile, runtimeShortcut } from "../model/overview";

function Action({ icon, title, hint, onClick, primary, disabled }: { icon: string; title: string; hint: ReactNode; onClick: () => void; primary?: boolean; disabled?: boolean }) {
  return (
    <button type="button" className={cx("hm-action", primary && "is-primary")} onClick={onClick} disabled={disabled}>
      <span className="hm-action-icon" aria-hidden="true"><Icon name={icon} size={18} /></span>
      <span className="hm-action-text">
        <span className="hm-action-title">{title}</span>
        <span className="hm-action-hint">{hint}</span>
      </span>
    </button>
  );
}

/** New session, download a model, start the runtime of the default profile, open the command palette. */
export function QuickActions() {
  const agents = useAgents();
  const profiles = useProfiles();
  const config = useConfig();
  const runtimes = useRuntimes();
  const createSession = useCreateSession();
  const runtimeAction = useRuntimeAction();

  const configured = config.data?.effective["agent.default"];
  const agentId = defaultAgentId(agents.data ?? [], configured);
  const agent = agents.data?.find((a) => a.id === agentId);
  const shortcut = runtimeShortcut(runtimes.data ?? [], defaultProfile(agents.data ?? [], profiles.data ?? [], configured));

  const startRuntime = () => {
    if (shortcut.kind === "start") runtimeAction.mutate({ runtime: shortcut.runtime, action: "start", model: shortcut.model });
    else if (shortcut.kind === "running") navigate(`/runtimes/${encodeURIComponent(shortcut.runtime.id)}`);
    else navigate("/runtimes");
  };

  return (
    <section className="hm-actions" aria-label="Azioni rapide">
      <Action
        primary icon="plus" title="Nuova sessione" hint={agent ? `con ${agent.name}` : "scegli un agente"}
        disabled={!agentId || createSession.isPending}
        onClick={() => createSession.mutate({ agent_id: agentId }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) })}
      />
      <Action icon="download" title="Scarica un modello" hint="Hugging Face, Ollama, catalogo" onClick={() => navigate("/models/discover")} />
      <Action
        icon="power"
        title={shortcut.kind === "start" ? `Avvia ${shortcut.runtime.name}` : shortcut.kind === "running" ? "Runtime in esecuzione" : "Avvia un runtime"}
        hint={shortcut.kind === "start" ? "runtime del profilo predefinito" : shortcut.kind === "running" ? shortcut.runtime.name : "apri l'elenco dei runtime"}
        disabled={runtimeAction.isPending}
        onClick={startRuntime}
      />
      <Action icon="search" title="Apri la palette" hint={<><kbd>Ctrl</kbd> <kbd>K</kbd> · comandi e pagine</>} onClick={openPalette} />
    </section>
  );
}
