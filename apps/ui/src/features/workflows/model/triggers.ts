import type { WorkflowRun, WorkflowTrigger } from "../../../shared/api/types";
import { describeCron, nextCronRun } from "./cron";

/** The scheduler never fires an interval faster than this (`seconds.max(30)` in schedule.rs). */
export const MIN_INTERVAL_S = 30;

export type IntervalUnit = "s" | "min" | "h";
export const UNIT_SECONDS: Record<IntervalUnit, number> = { s: 1, min: 60, h: 3600 };
export const UNIT_LABEL: Record<IntervalUnit, string> = { s: "secondi", min: "minuti", h: "ore" };

/** `21600` → `{ amount: 6, unit: "h" }`: the largest unit that divides the seconds evenly. */
export function splitInterval(seconds: number): { amount: number; unit: IntervalUnit } {
  if (seconds > 0 && seconds % 3600 === 0) return { amount: seconds / 3600, unit: "h" };
  if (seconds > 0 && seconds % 60 === 0) return { amount: seconds / 60, unit: "min" };
  return { amount: seconds, unit: "s" };
}

export const joinInterval = (amount: number, unit: IntervalUnit): number => Math.round(amount * UNIT_SECONDS[unit]);

export function intervalLabel(seconds: number): string {
  const { amount, unit } = splitInterval(seconds);
  return `Ogni ${amount} ${unit === "s" ? "s" : unit === "min" ? "min" : amount === 1 ? "ora" : "h"}`;
}

export const TRIGGER_KINDS: { kind: WorkflowTrigger["kind"]; label: string; hint: string }[] = [
  { kind: "manual", label: "Manuale", hint: "Parte solo quando premi «Esegui ora» o lo chiama un altro programma." },
  { kind: "cron", label: "Pianificato (cron)", hint: "A orari fissi, con la sintassi cron a 5 campi." },
  { kind: "interval", label: "A intervalli", hint: "Ogni tot minuti o ore, a partire dall'avvio del servizio." },
  { kind: "file", label: "Quando un file cambia", hint: "Controlla la data di modifica di un file o di una cartella." },
  { kind: "webhook", label: "Webhook", hint: "Parte quando un servizio esterno chiama l'indirizzo del workflow con il segreto." },
];

export function triggerLabel(t: WorkflowTrigger): string {
  switch (t.kind) {
    case "manual": return "Manuale";
    case "interval": return intervalLabel(t.seconds);
    case "cron": return describeCron(t.expr) ?? `Cron ${t.expr}`;
    case "file": return `Quando cambia ${t.path}`;
    case "webhook": return "Webhook";
  }
}

export interface TriggerSummary {
  label: string;
  /** Next scheduled start, when it can be known from here. */
  next?: Date;
  /** Why there is no next start, or how reliable it is. */
  note?: string;
}

/**
 * Label and, where possible, the next start. Cron is computed (the scheduler runs on local time);
 * an interval is only estimated from the last run, because the real timer starts with the service.
 * A disabled workflow has no next start.
 */
export function summarizeTrigger(t: WorkflowTrigger, opts: { enabled: boolean; lastRun?: Pick<WorkflowRun, "started_at">; now?: Date }): TriggerSummary {
  const label = triggerLabel(t);
  const now = opts.now ?? new Date();
  if (!opts.enabled) {
    return { label, note: t.kind === "manual" || t.kind === "webhook" ? undefined : "Disattivato: non parte da solo" };
  }
  switch (t.kind) {
    case "cron": {
      const next = nextCronRun(t.expr, now);
      return next ? { label, next } : { label, note: "Nessuna esecuzione prevista" };
    }
    case "interval": {
      const every = Math.max(MIN_INTERVAL_S, t.seconds) * 1000;
      const last = opts.lastRun ? new Date(opts.lastRun.started_at).getTime() : Number.NaN;
      if (Number.isNaN(last)) return { label, note: "Prima esecuzione dopo l'avvio del servizio" };
      let next = last + every;
      if (next <= now.getTime()) next += (Math.floor((now.getTime() - next) / every) + 1) * every;
      return { label, next: new Date(next), note: "stima dall'ultima esecuzione" };
    }
    default:
      return { label };
  }
}

/** Seconds between two ISO timestamps; null while the second is missing or either is invalid. */
export function runSeconds(startedAt: string, finishedAt: string | undefined, now: number = Date.now()): number | null {
  const a = new Date(startedAt).getTime();
  const b = finishedAt ? new Date(finishedAt).getTime() : now;
  return Number.isNaN(a) || Number.isNaN(b) ? null : Math.max(0, (b - a) / 1000);
}
