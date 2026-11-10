import type { InferenceProfile, LocalModel } from "../../../shared/api/types";

/** Quick actions of the simple Panoramica: only what a person does every day. */
export type SimpleActionId = "new-chat" | "download-model";

export interface SimpleAction {
  id: SimpleActionId;
  title: string;
  hint: string;
}

export const SIMPLE_ACTIONS: SimpleAction[] = [
  { id: "new-chat", title: "Nuova chat", hint: "inizia una conversazione" },
  { id: "download-model", title: "Scarica un modello", hint: "scegline uno dal catalogo" },
];

/** Human name of the model the default profile uses ("Il tuo assistente"); undefined when none is chosen or it is not on disk. */
export function assistantModelName(profile: InferenceProfile | undefined, models: LocalModel[]): string | undefined {
  if (!profile?.model) return undefined;
  const name = models.find((m) => m.id === profile.model)?.name.trim();
  return name || undefined;
}

export interface SimpleSections {
  /** The "Il tuo assistente" card. */
  assistant: boolean;
  /** Confirmations the agents wait for: shown only when there are some. */
  approvals: boolean;
  /** Recent conversations: shown once there is at least one. */
  recent: boolean;
  /** Lessons proposed by the agents: shown only when there are some to review. */
  lessons: boolean;
}

/** Which blocks of the simple Panoramica appear; empty ones stay out of the way. */
export function simpleSections(opts: { modelName?: string; approvals: number; sessions: number; lessons: number }): SimpleSections {
  return { assistant: !!opts.modelName, approvals: opts.approvals > 0, recent: opts.sessions > 0, lessons: opts.lessons > 0 };
}
