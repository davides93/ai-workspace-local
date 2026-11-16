import type { UiMode } from "../../lib/uiMode";
import { createChannel, updateChannel } from "./channels";
import { createContext } from "./contexts";
import { seedDraft } from "./drafts";
import { catalog, configLayers, folderGrants, models, nid, now, runtimes, sessionMessages, sessions, system } from "./fixtures";
import { taskScenario } from "./tasks";

/**
 * Named starting states of the demo, picked with `?scenario=<name>` in the address of the mockup.
 * The default demo is a well-used workspace; the end-to-end tests (`e2e/`) also need the other end,
 * so a scenario only empties what a new installation does not have yet. Nothing is random.
 *
 * - `first-run`: a runtime that is up, no model on disk, no conversation: what the first start of
 *   the simple interface looks like before "Prepara tutto".
 * - `mas`: the sandboxed Mac App Store build: the agents work in the app's own folder and the person grants more
 *   folders of the Mac (Download is already granted), so Settings shows «Cartelle consentite».
 * - `mail`: a Telegram channel and an email channel, both on, linked into one shared conversation, and four email drafts
 *   (two written by the assistant and waiting for the owner, one that failed, one already sent), so «Canali» shows the
 *   linked conversations and the review of the drafts.
 */
export const MOCK_SCENARIOS = ["first-run", "mas", "mail"] as const;
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
    // No chat, no background task: the demo tasks live in those sessions.
    taskScenario.empty = true;
  }
  if (name === "mail") {
    const tg = createChannel({ kind: "telegram", name: "Telegram", mode: "ask", allowed: ["111"], secrets: { bot_token: "123456:demo" } });
    const mail = createChannel({
      kind: "email", name: "Posta", mode: "ask", allowed: ["mario@example.it"],
      secrets: { imap_password: "demo", smtp_password: "demo" },
      settings: { imap_host: "imap.example.org", imap_user: "assistente", smtp_host: "smtp.example.org", from: "assistente@casa.example" },
    });
    updateChannel(tg.id, { enabled: true });
    updateChannel(mail.id, { enabled: true });
    createContext({ name: "Mario, ovunque", members: [{ channel_id: tg.id, sender: "111" }, { channel_id: mail.id, sender: "mario@example.it" }] });
    seedDraft({
      to: ["anna@cliente.it"], cc: ["capo@esempio.it"], bcc: ["archivio@esempio.it"], subject: "Preventivo per il cantiere",
      body: "Buongiorno Anna,\nin allegato il preventivo richiesto per il cantiere di via Roma.\n\nResto a disposizione.\nMario",
      in_reply_to: "<richiesta@cliente.it>", attachments: ["docs/preventivo-cantiere.pdf"], source: "mail:<richiesta@cliente.it>",
    }, "assistant");
    seedDraft({ to: ["luca@esempio.it"], subject: "Conferma riunione di giovedì", body: "Ciao Luca, confermo la riunione di giovedì alle 15.", source: "calendar:Riunione di giovedì" }, "assistant");
    seedDraft({ to: ["rotto@cliente.it"], subject: "Sollecito fattura", body: "Gentile cliente, ricordo la fattura in scadenza." }, "user", {
      status: "failed", note: "il server di posta ha rifiutato il destinatario rotto@cliente.it (550 no such user) (non è partito nulla)",
    });
    seedDraft({ to: ["bruno@esempio.it"], subject: "Grazie", body: "Grazie per l'incontro di ieri." }, "user", { status: "sent", sent_at: now(), message_id: "<draft-demo@aiw.local>" });
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
