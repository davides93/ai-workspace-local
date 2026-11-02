// Small memory of "where the user was" so that switching section keeps the context:
// the Workspace link of the rail, tab bar and palette reopens the last active chat session.
const KEY = "aiw.chat.last";
let lastChat: string | null = readStored();

function readStored(): string | null {
  try {
    return sessionStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function rememberChatSession(id: string | null): void {
  lastChat = id;
  try {
    if (id) sessionStorage.setItem(KEY, id);
    else sessionStorage.removeItem(KEY);
  } catch {
    /* storage unavailable: in-memory only */
  }
}

export const lastChatSession = (): string | null => lastChat;
