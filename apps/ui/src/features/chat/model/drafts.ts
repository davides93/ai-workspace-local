// Unsent message drafts per session. They live outside React so switching view (which unmounts
// the chat) or session does not lose what the user was typing.
const drafts = new Map<string, string>();

export const getDraft = (sessionId: string | null): string => (sessionId ? drafts.get(sessionId) ?? "" : "");

export function setDraft(sessionId: string | null, text: string): void {
  if (!sessionId) return;
  if (text) drafts.set(sessionId, text);
  else drafts.delete(sessionId);
}
