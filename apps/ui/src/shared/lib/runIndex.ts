// Which chat session a run belongs to. The backend names the run in audit entries and in
// confirmation requests, but not the session; the UI learns the pairing while it follows runs
// (the run started here, or replayed on re-attach) and remembers it for the tab's lifetime.
const KEY = "aiw.runs";
const MAX = 400;

const runs = new Map<string, string>(read());

function read(): [string, string][] {
  try {
    const raw = sessionStorage.getItem(KEY);
    const v: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(v) ? v.filter((e): e is [string, string] => Array.isArray(e) && typeof e[0] === "string" && typeof e[1] === "string") : [];
  } catch {
    return [];
  }
}

function persist() {
  try {
    sessionStorage.setItem(KEY, JSON.stringify([...runs]));
  } catch {
    /* storage unavailable: in-memory only */
  }
}

export function rememberRun(runId: string, sessionId: string): void {
  if (!runId || !sessionId || runs.get(runId) === sessionId) return;
  runs.delete(runId);
  runs.set(runId, sessionId);
  while (runs.size > MAX) runs.delete(runs.keys().next().value as string);
  persist();
}

/** Session of a run, when this tab has seen it. */
export const sessionForRun = (runId: string | null | undefined): string | undefined => (runId ? runs.get(runId) : undefined);

/** Where to open the conversation of a run, when known. */
export const chatPathForRun = (runId: string | null | undefined): string | undefined => {
  const s = sessionForRun(runId);
  return s ? `/chat/${encodeURIComponent(s)}` : undefined;
};

/** Forgets every run of a session (it was deleted). */
export function forgetSessionRuns(sessionId: string): void {
  let changed = false;
  for (const [r, s] of runs) {
    if (s === sessionId) {
      runs.delete(r);
      changed = true;
    }
  }
  if (changed) persist();
}
