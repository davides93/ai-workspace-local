// Text that something outside the chat wants in the message box (the command palette's «Scarica un modello», in the
// simple interface): `seedComposer` keeps it until a conversation is open, `takeComposerSeed` hands it over once. The chat
// listens (`onComposerSeed`) so that it picks the text up whether it is already on screen or still loading.

let pending: string | null = null;
const listeners = new Set<() => void>();

/** Asks for `text` to be put in the message box of the open conversation (it waits for one if none is open yet). */
export function seedComposer(text: string): void {
  pending = text;
  for (const l of [...listeners]) l();
}

/** The waiting text, once; `null` when there is none. */
export function takeComposerSeed(): string | null {
  const text = pending;
  pending = null;
  return text;
}

/** The chat's side: `handler` runs on every `seedComposer()`. Returns the function that unsubscribes. */
export function onComposerSeed(handler: () => void): () => void {
  listeners.add(handler);
  return () => void listeners.delete(handler);
}

/** The text to show after `seed` is added to what is already typed: on a new line below it, or alone when the box is empty. */
export const withSeed = (draft: string, seed: string): string => (draft.trim() ? `${draft.trimEnd()}\n${seed}` : seed);
