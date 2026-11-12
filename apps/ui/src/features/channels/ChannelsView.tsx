import { useState } from "react";
import { Button, EmptyState, Page, PageHeader, QueryState } from "../../shared/ui";
import { useAgents } from "../agents/api";
import { AddChannel } from "./components/AddChannel";
import { ChannelCard } from "./components/ChannelCard";
import { useChannels } from "./api";
import { ALWAYS_ON_NOTE } from "./model/channels";
import "./channels.css";

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
        eyebrow="Lavoro" title="Canali"
        subtitle="Parla con l'assistente da Telegram, Slack o email. Risponde solo a chi hai scritto nella lista; tutti gli altri vengono ignorati."
        actions={!adding && <Button variant="primary" icon="plus" onClick={() => setAdding(true)}>Aggiungi un canale</Button>}
      />
      <p className="muted text-sm">{ALWAYS_ON_NOTE}</p>
      {adding && <AddChannel onDone={() => setAdding(false)} />}
      <QueryState query={channels}>
        {(list) =>
          list.length === 0 && !adding ? (
            <EmptyState icon="send" title="Nessun canale collegato" description="Collega Telegram, Slack o una casella email per scrivere all'assistente anche quando non sei davanti al computer." action={<Button variant="primary" onClick={() => setAdding(true)}>Aggiungi un canale</Button>} />
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
