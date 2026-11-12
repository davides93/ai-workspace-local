import { useState } from "react";
import type { McpServer } from "../../../shared/api/types";
import { Button, Dialog, EmptyState, Icon, QueryState, Section } from "../../../shared/ui";
import { useMcpServers, useMcpSignIn, useMcpSignOut, useReconnectMcp, useRemoveMcpServer, useSaveMcpServer, useSetToolsEnabled, useTools } from "../api";
import { draftFromServer, draftToPayload } from "../model/mcpForm";
import { McpServerCard } from "./McpServerCard";
import { McpServerDrawer } from "./McpServerDrawer";

/** MCP servers: list with state and tools, add/edit drawer, remove with confirmation. */
export function McpTab() {
  const servers = useMcpServers();
  const tools = useTools();
  const save = useSaveMcpServer();
  const remove = useRemoveMcpServer();
  const setTools = useSetToolsEnabled();
  const reconnect = useReconnectMcp();
  const signIn = useMcpSignIn();
  const signOut = useMcpSignOut();
  // `"new"` opens an empty form, a server opens it for editing.
  const [editing, setEditing] = useState<McpServer | "new" | null>(null);
  const [removing, setRemoving] = useState<McpServer | null>(null);

  const toggleServer = (s: McpServer, enabled: boolean) => save.mutate(draftToPayload({ ...draftFromServer(s), enabled }));

  return (
    <Section
      title="Server MCP"
      hint={<>stdio o streamable HTTP; i loro tool diventano <code>mcp__server__tool</code>.</>}
      actions={<Button variant="primary" icon="plus" onClick={() => setEditing("new")}>Aggiungi server</Button>}
    >
      <div className="panel cap-notice">
        <div className="row nowrap cap-notice-body">
          <Icon name="alert" size={20} />
          <p className="text-sm">
            <b>I server stdio girano fuori dalla sandbox.</b> Sono processi avviati con i tuoi permessi: la sandbox degli agenti non li limita.
            Aggiungi solo comandi di cui ti fidi; i tool che espongono restano soggetti alla conferma in base al rischio.
          </p>
        </div>
      </div>

      <QueryState
        query={servers}
        isEmpty={(l) => l.length === 0}
        empty={(
          <EmptyState
            icon="plug" title="Nessun server MCP"
            description="Un server MCP aggiunge tool agli agenti: file, GitHub, database, browser. Per esempio npx -y @modelcontextprotocol/server-filesystem."
            action={<Button variant="primary" icon="plus" onClick={() => setEditing("new")}>Aggiungi il primo server</Button>}
          />
        )}
      >
        {(list) => (
          <div className="stack stack-lg">
            {list.map((s) => (
              <McpServerCard
                key={s.id} server={s} tools={tools.data ?? []}
                onToggleServer={(enabled) => toggleServer(s, enabled)}
                onToggleTool={(name, enabled) => setTools.mutate({ names: [name], enabled })}
                onEdit={() => setEditing(s)} onRemove={() => setRemoving(s)}
                onReconnect={() => reconnect.mutate(s)} onSignIn={() => signIn.mutate(s)} onSignOut={() => signOut.mutate(s)}
                busy={reconnect.isPending || signIn.isPending || signOut.isPending}
              />
            ))}
          </div>
        )}
      </QueryState>

      {editing && (
        <McpServerDrawer
          key={editing === "new" ? "new" : editing.id}
          server={editing === "new" ? null : editing}
          others={(servers.data ?? []).filter((s) => editing === "new" || s.id !== editing.id).map((s) => s.name)}
          saving={save.isPending}
          onSave={(payload) => save.mutate(payload, { onSuccess: () => setEditing(null) })}
          onClose={() => setEditing(null)}
        />
      )}
      {removing && (
        <Dialog
          title="Rimuovi il server MCP" size="sm" onClose={() => setRemoving(null)}
          footer={<>
            <Button onClick={() => setRemoving(null)}>Annulla</Button>
            <Button variant="danger" icon="trash" disabled={remove.isPending} onClick={() => remove.mutate(removing, { onSettled: () => setRemoving(null) })}>Rimuovi</Button>
          </>}
        >
          <p>Vuoi rimuovere <b>{removing.name}</b>? Il server viene scollegato e i suoi {removing.tools.length} tool non saranno più disponibili agli agenti. Potrai aggiungerlo di nuovo.</p>
        </Dialog>
      )}
    </Section>
  );
}
