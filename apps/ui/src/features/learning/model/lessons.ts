import { MAX_LESSON_CONTENT_CHARS, MAX_LESSON_TITLE_CHARS, type Lesson, type LessonKind } from "../../../shared/api/types";

import { t as tr } from "../../../shared/i18n";
export const KIND_LABEL: Record<LessonKind, string> = { get memory() { return tr("learning.memoria"); }, skill: "Skill" };

/** Where an approved lesson ends up, in words the user can check. */
export const KIND_DESTINATION: Record<LessonKind, string> = {
  get memory() { return tr("learning.verra_aggiunta_a_aiw_agents"); },
  get skill() { return tr("learning.diventera_una_nuova_skill_in"); },
};

/** The toast after an approval. */
export const approvedMessage = (l: Pick<Lesson, "kind" | "title">): string =>
  l.kind === "skill" ? tr("learning.lezione_approvata_e_una_nuova", { title: l.title }) : tr("learning.lezione_approvata_e_in_agents", { title: l.title });

export type LessonEditCheck = { ok: true; title: string; content: string } | { ok: false; error: string };

/** The same checks as the engine, so the form says what is wrong before the request. */
export function checkLessonEdit(title: string, content: string): LessonEditCheck {
  const t = title.trim();
  const c = content.trim();
  if (!t) return { ok: false, error: tr("learning.il_titolo_non_puo_essere") };
  if (!c) return { ok: false, error: tr("learning.il_contenuto_non_puo_essere") };
  if ([...t].length > MAX_LESSON_TITLE_CHARS) return { ok: false, error: tr("learning.il_titolo_supera_i_caratteri", { max: MAX_LESSON_TITLE_CHARS }) };
  if ([...c].length > MAX_LESSON_CONTENT_CHARS) return { ok: false, error: tr("learning.il_contenuto_supera_i_caratteri", { max: MAX_LESSON_CONTENT_CHARS }) };
  return { ok: true, title: t, content: c };
}

/** Only what the user changed goes to the engine; nothing when the lesson is approved as it is. */
export function editOf(lesson: Pick<Lesson, "title" | "content">, title: string, content: string): { title?: string; content?: string } {
  const edit: { title?: string; content?: string } = {};
  if (title.trim() !== lesson.title) edit.title = title.trim();
  if (content.trim() !== lesson.content) edit.content = content.trim();
  return edit;
}

/** The lesson waiting for review for this session, if any. */
export const pendingFor = (lessons: Lesson[] | undefined, sessionId: string | null): Lesson | undefined =>
  sessionId ? lessons?.find((l) => l.session_id === sessionId && l.status === "pending") : undefined;

/** A decided lesson replaces the old one in a cached list; a pending-only list drops it. */
export function withDecided(list: Lesson[] | undefined, lesson: Lesson, pendingOnly: boolean): Lesson[] {
  const rest = (list ?? []).filter((l) => l.id !== lesson.id);
  return pendingOnly && lesson.status !== "pending" ? rest : [lesson, ...rest];
}

/** The memory lesson waiting for an answer in this session: the one the "Vuoi che lo ricordi?" card shows (skills stay in the Panoramica). */
export const memoryPendingFor = (lessons: Lesson[] | undefined, sessionId: string | null): Lesson | undefined =>
  sessionId ? lessons?.find((l) => l.session_id === sessionId && l.status === "pending" && l.kind === "memory") : undefined;

/** The sentence the card shows: the content of the lesson on one line, cut when it is long. */
export function rememberText(l: Pick<Lesson, "title" | "content">): string {
  const one = l.content.replace(/\s+/g, " ").trim() || l.title;
  return one.length > 220 ? `${one.slice(0, 220).trimEnd()}…` : one;
}
