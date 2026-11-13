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
import { SIMPLE_ACTIONS } from "../model/simple";

import { t, t as tr, tNode as trNode } from "../../../shared/i18n";
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

/** New session, download a model, start the runtime of the default profile, open the command palette. Simple mode: new chat and download only. */
export function QuickActions({ simple = false }: { simple?: boolean }) {
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

  if (simple) {
    const [chat, download] = SIMPLE_ACTIONS;
    return (
      <section className="hm-actions hm-actions-simple" aria-label={t("home.azioni_rapide")}>
        <Action
          primary icon="plus" title={chat.title} hint={chat.hint}
          disabled={!agentId || createSession.isPending}
          onClick={() => createSession.mutate({ agent_id: agentId }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) })}
        />
        <Action icon="download" title={download.title} hint={download.hint} onClick={() => navigate("/models/discover")} />
      </section>
    );
  }

  return (
    <section className="hm-actions" aria-label={t("home.azioni_rapide")}>
      <Action
        primary icon="plus" title={t("home.nuova_sessione")} hint={agent ? t("home.con", { name: agent.name }) : t("home.scegli_un_agente")}
        disabled={!agentId || createSession.isPending}
        onClick={() => createSession.mutate({ agent_id: agentId }, { onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`) })}
      />
      <Action icon="download" title={t("home.scarica_un_modello")} hint={t("home.hugging_face_ollama_catalogo")} onClick={() => navigate("/models/discover")} />
      <Action
        icon="power"
        title={shortcut.kind === "start" ? t("home.avvia", { name: shortcut.runtime.name }) : shortcut.kind === "running" ? t("home.runtime_in_esecuzione") : t("home.avvia_un_runtime")}
        hint={shortcut.kind === "start" ? t("home.runtime_del_profilo_predefinito") : shortcut.kind === "running" ? shortcut.runtime.name : t("home.apri_l_elenco_dei_runtime")}
        disabled={runtimeAction.isPending}
        onClick={startRuntime}
      />
      <Action icon="search" title={t("home.apri_la_palette")} hint={<>{trNode("home.comandi_e_pagine", { kbd: <kbd>{tr("home.ctrl")}</kbd>, kbd2: <kbd>{tr("home.k")}</kbd> })}</>} onClick={openPalette} />
    </section>
  );
}
