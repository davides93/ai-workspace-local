/**
 * Does this tool answer say a path is outside the folders the agent may use? Then, in the Mac App Store build, the person can
 * allow that folder in Settings (the agent cannot): the card says where. Matches the wording of `ToolContext::resolve`
 * («… è fuori dal workspace (…)», «… punta fuori dal workspace tramite un collegamento simbolico …»), whatever prefix the run
 * puts before it («Negato: …»); a path hidden for holding secrets or the app's data is another message and gets no hint.
 */
export function isOutsideWorkspace(result: string | undefined): boolean {
  return result !== undefined && /fuori dal workspace/i.test(result) && !/nascosto agli agenti/i.test(result);
}
