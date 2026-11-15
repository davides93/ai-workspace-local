// Opens the menu (the sheet with device, search, mode, language, theme and the kill switch) from anywhere, like
// `openPalette()` does for the palette: the chat header has a «Menu» button on phones, where the simple interface has
// neither rail nor tab bar. The shell subscribes once (`onOpenMenu` in App).

const listeners = new Set<() => void>();

/** Asks the shell to open the menu. A no-op when the shell is not mounted. */
export function openMenu(): void {
  for (const l of [...listeners]) l();
}

/** The shell's side: `handler` runs on every `openMenu()`. Returns the function that unsubscribes. */
export function onOpenMenu(handler: () => void): () => void {
  listeners.add(handler);
  return () => void listeners.delete(handler);
}
