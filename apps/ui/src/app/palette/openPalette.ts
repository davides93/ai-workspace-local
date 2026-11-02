// Opens the command palette from anywhere (the Panoramica tile, the rail, the "Altro" sheet) without faking a
// Ctrl+K key press. The shell subscribes once (`onOpenPalette` in App); everyone else calls `openPalette()`.

const listeners = new Set<() => void>();

/** Asks the shell to open the palette. A no-op when the shell is not mounted. */
export function openPalette(): void {
  for (const l of [...listeners]) l();
}

/** The shell's side: `handler` runs on every `openPalette()`. Returns the function that unsubscribes. */
export function onOpenPalette(handler: () => void): () => void {
  listeners.add(handler);
  return () => void listeners.delete(handler);
}
