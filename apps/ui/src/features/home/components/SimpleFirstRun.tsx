import { useEffect, useRef, useState } from "react";
import { LanguageSelect } from "../../../app/LanguageSelect";
import { navigate } from "../../../app/router";
import { useUiMode } from "../../../app/useUiMode";
import { defaultAgentId } from "../../../shared/lib/agents";
import { errorMessage } from "../../../shared/lib/errors";
import { formatBytes, formatPercent } from "../../../shared/lib/format";
import { Button, Meter, Panel } from "../../../shared/ui";
import type { InferenceProfile, LocalModel, RuntimeInfo, SystemInfo } from "../../../shared/api/types";
import { useAgents, useSaveProfile } from "../../agents/api";
import { useCreateSession } from "../../chat/api";
import { isActiveDownload, useDownloads, usePullModel } from "../../models/api";
import { useConfig } from "../../settings/api";
import { downloadFraction, isUsableProfile, profileForModel, type SimpleFirstRun } from "../model/overview";

import { t, t as tr } from "../../../shared/i18n";
/**
 * First run of the simple interface: one sentence and one button. "Prepara tutto" downloads the
 * model the adviser recommends (progress stays here), makes sure the default agent uses it once it
 * is on disk (nothing to do with `auto:agent`, the default profile is patched otherwise) and opens a new chat. With a model already ready the button is just "Inizia una chat".
 */
export function SimpleFirstRunPanel({ state, models, runtimes, profile, device, followsAdviser, onDismiss }: {
  state: SimpleFirstRun;
  models: LocalModel[];
  runtimes: RuntimeInfo[];
  profile?: InferenceProfile;
  device?: SystemInfo["device"];
  /** The default agent's profile is `auto:*`: it picks up the downloaded model by itself. */
  followsAdviser?: boolean;
  onDismiss: () => void;
}) {
  const [, setMode] = useUiMode();
  const downloads = useDownloads();
  const pull = usePullModel({ handled: true });
  const saveProfile = useSaveProfile();
  const createSession = useCreateSession();
  const agents = useAgents();
  const config = useConfig();
  const [working, setWorking] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const started = useRef(false);

  const agentId = defaultAgentId(agents.data ?? [], config.data?.effective["agent.default"]);
  const rec = state.kind === "prepare" ? state.model : undefined;
  const job = rec ? downloads.data?.find((d) => d.model_id === rec.id && isActiveDownload(d)) : undefined;
  const lastJob = rec ? downloads.data?.find((d) => d.model_id === rec.id) : undefined;

  const openChat = () =>
    createSession.mutate({ agent_id: agentId }, {
      onSuccess: (s) => navigate(`/chat/${encodeURIComponent(s.id)}`),
      onError: (e) => { setFailure(errorMessage(e)); setWorking(false); started.current = false; },
    });

  // Once the model is on disk, chat. An agent on `auto:*` already follows it; only an existing install
  // whose agent points at a stored profile needs that profile pointed at the model first.
  useEffect(() => {
    if (!working || started.current || !agentId) return;
    const target = state.kind === "connect" ? { model: state.model, runtimeId: state.runtimeId } : state.kind === "chat" ? "chat" : undefined;
    if (!target) return;
    started.current = true;
    if (target === "chat" || followsAdviser || isUsableProfile(profile, models, runtimes)) return openChat();
    saveProfile.mutate(profileForModel(profile, target.model, target.runtimeId), {
      onSuccess: openChat,
      onError: (e) => { setFailure(errorMessage(e)); setWorking(false); started.current = false; },
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [working, state, agentId]);

  const failedDownload = working && !job && lastJob?.status === "error" ? lastJob.error ?? t("home.download_non_riuscito") : null;

  const go = () => {
    setFailure(null);
    if (state.kind === "prepare") {
      pull.mutate(state.model.id, { onSuccess: () => setWorking(true), onError: (e) => setFailure(errorMessage(e)) });
    } else {
      setWorking(true);
    }
  };

  let sentence: string;
  let action: "go" | "developer" | "models" | "none" = "go";
  switch (state.kind) {
    case "chat": sentence = t("home.il_tuo_assistente_e_pronto"); break;
    case "prepare": sentence = t(device ? "home.ti_consiglio_per_dispositivo" : "home.ti_consiglio_per_questo_dispositivo", { model: state.model.name, size: formatBytes(state.model.size_bytes), device: device?.name ?? "", detail: state.model.fit_detail }); break;
    case "connect": sentence = t("home.hai_gia_sul_disco_lo", { name: state.model.name }); break;
    case "no-runtime": sentence = t("home.su_questo_dispositivo_non_c"); action = "developer"; break;
    case "no-model": sentence = t("home.nessun_modello_del_catalogo_e"); action = "models"; break;
    case "loading": sentence = t("home.cerco_il_modello_piu_adatto"); action = "none"; break;
  }

  const busy = working || pull.isPending || saveProfile.isPending || createSession.isPending;
  const label = state.kind === "chat" ? t("home.inizia_una_chat") : t("home.prepara_tutto");
  const error = failure ?? failedDownload;

  return (
    <Panel
      title={t("home.per_iniziare")} icon="sparkle" className="hm-firstrun"
      actions={<Button size="sm" variant="ghost" onClick={onDismiss} title={tr("home.nasconde_questo_riquadro")}>{t("home.nascondi")}</Button>}
    >
      <div className="stack stack-lg">
        <LanguageSelect />
        <p className="hm-simple-text">{sentence}</p>
        {job && (
          <span className="hm-step-progress" role="status">
            <span>{t("home.scarico", { p: rec?.name ?? job.model_ref, p2: formatPercent(downloadFraction(job.received, job.total)) })}</span>
            <Meter label={t("home.avanzamento_di", { p: rec?.name ?? job.model_ref })} value={job.received} max={job.total} />
          </span>
        )}
        {working && !job && !error && <span className="faint text-sm" role="status">{t("home.preparo_la_chat")}</span>}
        {error && <p className="mh-alert warn" role="alert">{error}</p>}
        <div className="row">
          {action === "go" && <Button variant="primary" icon={state.kind === "chat" ? "plus" : "download"} disabled={busy && !error} onClick={go}>{error && working ? t("home.riprova") : label}</Button>}
          {action === "developer" && <Button onClick={() => setMode("developer")}>{t("home.mostra_tutte_le_sezioni")}</Button>}
          {action === "models" && <Button variant="primary" onClick={() => navigate("/models/discover")}>{t("home.cerca_un_modello")}</Button>}
        </div>
      </div>
    </Panel>
  );
}
