import type { ReactNode } from "react";
import { EmptyState, SkeletonLines } from "../../../shared/ui";
import { SimpleFirstRunPanel } from "../../home/components/SimpleFirstRun";
import { useSimpleFirstRun } from "../../home/useSimpleFirstRun";
import { ChatEmpty } from "./ChatEmpty";

import { t } from "../../../shared/i18n";
/**
 * What the simple chat shows where an empty conversation (or no conversation at all) would be: the «Per iniziare» panel
 * while nothing can answer yet (no model, no engine, no session to start from), else the usual starting ideas. It is
 * the one way out that does not need the assistant, together with «Mostra tutte le sezioni».
 *
 * With no session at all the panel is shown whatever the state is: its button («Inizia una chat») creates the first one.
 * `fallback` is the plain empty state to use if the data it needs could not be read.
 */
export function SimpleEmpty({ noSession, onPick, fallback }: { noSession: boolean; onPick: (text: string) => void; fallback?: ReactNode }) {
  const first = useSimpleFirstRun();
  if (!first.state) {
    if (first.failed) return <>{fallback ?? <ChatEmpty simple onPick={onPick} />}</>;
    return (
      <div className="ws-firstrun" role="status" aria-label={t("chat.carico_la_conversazione")}>
        <SkeletonLines lines={3} />
      </div>
    );
  }
  if (noSession || first.state.kind !== "chat") {
    return (
      <div className="ws-firstrun">
        <SimpleFirstRunPanel
          state={first.state} models={first.models} runtimes={first.runtimes} profile={first.profile} device={first.device}
          followsAdviser={first.followsAdviser}
        />
      </div>
    );
  }
  return <ChatEmpty simple onPick={onPick} />;
}

/** The plain empty state of a chat with no session, for when the first-run data cannot be read (the button creates the first session). */
export function NoSessionState({ action }: { action: ReactNode }) {
  return <EmptyState icon="chat" title={t("chat.nessuna_sessione")} description={t("chat.crea_la_prima_sessione_per")} action={action} />;
}
