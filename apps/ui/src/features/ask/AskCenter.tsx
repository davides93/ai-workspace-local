import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Link, useHash } from "../../app/router";
import { sessionForRun } from "../../shared/lib/runIndex";
import { qk } from "../../shared/query/keys";
import { Icon } from "../../shared/ui";
import { useSessions } from "../chat/api";
import { useQuestions, useSecretRequests } from "./api";
import { bannerSession, openChatSession, waitingElsewhere } from "./model";
import "./ask.css";

import { t } from "../../shared/i18n";
/**
 * The questions and the requests for a secret of agents that wait while the person is somewhere else: a banner that leads to
 * the conversation they belong to. The questions of the open conversation are cards in the conversation itself, so they are
 * left out here; the banner never has a field for a secret, only a way to the card that has it.
 */
export function AskCenter() {
  const questions = useQuestions().data;
  const secrets = useSecretRequests().data;
  const sessions = useSessions().data;
  const open = openChatSession(useHash());
  const q = waitingElsewhere(questions, open, sessionForRun);
  const s = waitingElsewhere(secrets, open, sessionForRun);
  const target = q.length + s.length > 0 ? bannerSession([...q, ...s], sessionForRun) : null;
  // The title the banner names may be older than the run that is waiting (a conversation is named after its first message).
  const qc = useQueryClient();
  useEffect(() => {
    if (target) void qc.invalidateQueries({ queryKey: qk.sessions.list() });
  }, [target, qc]);
  if (q.length + s.length === 0) return null;
  const title = target ? sessions?.find((x) => x.id === target)?.title : undefined;
  return (
    <div className="ask-banner" role="status">
      <Icon name={q.length > 0 ? "chat" : "lock"} size={16} />
      <div className="ask-banner-text">
        {q.length > 0 && <b>{t("ask.banner_domande", { n: q.length })}</b>}
        {s.length > 0 && <b>{t("ask.banner_segreti", { n: s.length })}</b>}
        <span>{target ? t("ask.banner_altra_sessione", { title: title ?? target }) : t("ask.banner_sessione_sconosciuta")}</span>
      </div>
      <Link to={target ? `/chat/${encodeURIComponent(target)}` : "/chat"} className="ask-banner-link">
        {target ? t("ask.apri_la_sessione") : t("ask.apri_la_chat")} <Icon name="forward" size={12} />
      </Link>
    </div>
  );
}
