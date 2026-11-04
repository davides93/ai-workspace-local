import { useSyncExternalStore } from "react";

export type ToastTone = "crit";
export interface ToastItem {
  id: number;
  text: string;
  tone?: ToastTone;
}

let items: ToastItem[] = [];
const listeners = new Set<() => void>();
let seq = 0;

function emit() {
  for (const l of listeners) l();
}

/** Shows a transient message. Errors use `tone: "crit"` and are announced as alerts. */
export function toast(text: string, tone?: ToastTone): void {
  const id = ++seq;
  items = [...items, { id, text, tone }];
  emit();
  setTimeout(() => dismissToast(id), tone === "crit" ? 6000 : 3800);
}

export function dismissToast(id: number): void {
  const next = items.filter((t) => t.id !== id);
  if (next.length === items.length) return;
  items = next;
  emit();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function useToasts(): ToastItem[] {
  return useSyncExternalStore(subscribe, () => items, () => items);
}
