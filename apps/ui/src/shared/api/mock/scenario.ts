import type { UiMode } from "../../lib/uiMode";
import { ApiError } from "../http";
import type { Transport } from "../transport";
import { catalog, configLayers, folderGrants, models, nid, now, runtimes, sessionMessages, sessions, system } from "./fixtures";
import { LONG_CHAT_ID, longChatMessages } from "./long-chat";

/**
 * Named starting states of the demo, picked with `?scenario=<name>` in the address of the mockup.
 * The default demo is a well-used workspace; the end-to-end tests (`e2e/`) also need the other end,
 * so a scenario only empties what a new installation does not have yet. Nothing is random.
 *
 * - `first-run`: a runtime that is up, no model on disk, no conversation: what the first start of
 *   the simple interface looks like before "Prepara tutto".
 * - `long-chat`: the default demo plus one very long conversation (`ses_long`, about four hundred rows with code in every language of the
 *   highlighter, tool cards, a table and a list): what a person who uses the app for months has. It is the first session of the list.
 * - `offline`: the workspace does not answer (every request fails with a 503): the connection banner and the error state of every view.
 * - `unpaired`: this browser has no token on a device that wants one (every request fails with a 401 until the pairing code `123456`
 *   is entered): the pairing screen, then the app. A wrong code is refused.
 * - `mas`: the sandboxed Mac App Store build: the agents work in the app's own folder and the person grants more
 *   folders of the Mac (Download is already granted), so Settings shows «Cartelle consentite».
 */
export const MOCK_SCENARIOS = ["first-run", "mas", "long-chat", "offline", "unpaired"] as const;
export type MockScenario = (typeof MOCK_SCENARIOS)[number];

export const isMockScenario = (v: unknown): v is MockScenario => (MOCK_SCENARIOS as readonly unknown[]).includes(v);

/** Mutates the demo fixtures in place; call once, before the transport answers its first request. */
export function applyMockScenario(name: MockScenario): void {
  if (name === "first-run") {
    models.length = 0;
    for (const c of catalog) c.installed = false;
    for (const r of runtimes) r.loaded_models = [];
    sessions.length = 0;
    for (const id of Object.keys(sessionMessages)) delete sessionMessages[id];
  }
  if (name === "long-chat") {
    sessions.unshift({ id: LONG_CHAT_ID, title: "Domande di programmazione (lunga)", agent_id: "coder", created_at: now(), updated_at: now(), last_status: "succeeded" });
    sessionMessages[LONG_CHAT_ID] = longChatMessages();
  }
  if (name === "mas") {
    system.sandboxed = true;
    system.folder_access = true;
    system.shell_sandbox = undefined;
    system.workspace_root = "/Users/demo/Library/Containers/dev.aiworkspace.local/Data/Documents/AIW-Workspace";
    system.device.os = "macos";
    folderGrants.push({ id: nid("fld"), name: "Download", path: "/Users/demo/Downloads", available: true, added: now() });
    // A conversation where the agent met the wall: the card of the refused tool says where the person can allow the folder.
    const refused = "Negato: /Users/demo/Library/Containers/dev.aiworkspace.local/Data/Downloads è fuori dal workspace (/Users/demo/Library/Containers/dev.aiworkspace.local/Data/Documents/AIW-Workspace). Se serve lavorare lì, solo l'utente può dare accesso a una cartella del computer: da Impostazioni → Cartelle consentite (non si fa dalla chat né dall'agente)";
    sessions.unshift({ id: "ses_mas", title: "Guarda la cartella Download", agent_id: "operator", created_at: now(), updated_at: now(), last_status: "succeeded" });
    sessionMessages.ses_mas = [
      { id: "mas1", role: "user", content: "Cosa c'è nella cartella Download?", created_at: now() },
      { id: "mas2", role: "assistant", content: "", tool_calls: [{ id: "c_mas", name: "list_dir", arguments: JSON.stringify({ path: "~/Downloads" }) }], created_at: now() },
      { id: "mas3", role: "tool", tool_call_id: "c_mas", name: "list_dir", content: refused, created_at: now() },
      { id: "mas4", role: "assistant", content: "Non posso guardare quella cartella: è fuori dal workspace. Puoi consentirla tu da **Impostazioni → Cartelle consentite**.", created_at: now() },
    ];
  }
}

/**
 * `?ui=developer` in the address of the mockup: the demo starts with the interface in Developer mode (the configuration
 * says `ui.mode: developer`, so every section is there and nothing redirects). Without it the demo is the simple
 * interface, which has the chat only. The end-to-end tests and the smoke test use it to reach every route.
 */
export function applyMockUiMode(mode: UiMode): void {
  const global = configLayers.find((l) => l.name === "global");
  if (global) global.values = { ...global.values, "ui.mode": mode };
}

/** The code the `unpaired` scenario accepts. */
export const DEMO_PAIRING_CODE = "123456";

/**
 * The transport as a scenario wants it. Most scenarios are data (`applyMockScenario`) and the transport is the demo's; `offline` and
 * `unpaired` are about the connection, so every request of the demo goes through a wrapper that fails the way the real one would.
 * `subscribeRun` is the one synchronous member of the interface: it is not a request and is never refused.
 */
export function transportForScenario(demo: Transport, name: MockScenario | null): Transport {
  if (name !== "offline" && name !== "unpaired") return demo;
  let paired = name !== "unpaired";
  const refusal = () => (name === "offline" ? new ApiError(503, "il workspace non risponde") : new ApiError(401, "serve il pairing: inserisci il codice mostrato sull'altro dispositivo"));
  return new Proxy(demo, {
    get(target, prop) {
      if (name === "unpaired") {
        if (prop === "capabilities") return { ...target.capabilities, pairing: true };
        if (prop === "pair") {
          return async (code: string): Promise<void> => {
            if (code.replace(/\D/g, "") !== DEMO_PAIRING_CODE) throw new ApiError(400, "codice non valido o scaduto");
            paired = true;
          };
        }
      }
      const value = Reflect.get(target, prop, target) as unknown;
      if (typeof value !== "function" || prop === "subscribeRun") return typeof value === "function" ? value.bind(target) : value;
      return (...args: unknown[]) => (paired && name === "unpaired") ? (value as (...a: unknown[]) => unknown).apply(target, args) : Promise.reject(refusal());
    },
  });
}
