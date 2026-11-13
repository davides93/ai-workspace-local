import { useState } from "react";
import { Button, EmptyState, Page, PageHeader, QueryState } from "../../shared/ui";
import { useAgents } from "../agents/api";
import { AddChannel } from "./components/AddChannel";
import { ChannelCard } from "./components/ChannelCard";
import { useChannels } from "./api";
import { alwaysOnNote } from "./model/channels";
import "./channels.css";

import { t } from "../../shared/i18n";
/**
 * `#/channels` — "Canali": talk to the assistant from Telegram, Slack or email. Off by default; only the people
 * on the list are heard; by default the assistant can only read.
 */
export function ChannelsView() {
  const channels = useChannels();
  const agents = useAgents();
  const [adding, setAdding] = useState(false);
  return (
    <Page>
      <PageHeader
        eyebrow={t("nav.group_work")} title={t("channels.canali")}
        subtitle={t("channels.parla_con_l_assistente_da")}
        actions={!adding && <Button variant="primary" icon="plus" onClick={() => setAdding(true)}>{t("channels.aggiungi_un_canale")}</Button>}
      />
      <p className="muted text-sm">{alwaysOnNote()}</p>
      {adding && <AddChannel onDone={() => setAdding(false)} />}
      <QueryState query={channels}>
        {(list) =>
          list.length === 0 && !adding ? (
            <EmptyState icon="send" title={t("channels.nessun_canale_collegato")} description={t("channels.collega_telegram_slack_o_una")} action={<Button variant="primary" onClick={() => setAdding(true)}>{t("channels.aggiungi_un_canale")}</Button>} />
          ) : (
            <div className="stack stack-lg">
              {list.map((c) => <ChannelCard key={c.id} channel={c} agents={agents.data ?? []} />)}
            </div>
          )
        }
      </QueryState>
    </Page>
  );
}
