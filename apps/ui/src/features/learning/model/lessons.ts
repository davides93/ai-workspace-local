import { MAX_LESSON_CONTENT_CHARS, MAX_LESSON_TITLE_CHARS, type Lesson, type LessonKind } from "../../../shared/api/types";

export const KIND_LABEL: Record<LessonKind, string> = { memory: "Memoria", skill: "Skill" };

/** Where an approved lesson ends up, in words the user can check. */
export const KIND_DESTINATION: Record<LessonKind, string> = {
  memory: "Verrà aggiunta a ~/.aiw/AGENTS.md, sotto «Lezioni apprese».",
  skill: "Diventerà una nuova skill in ~/.aiw/skills.",
};

/** The toast after an approval. */
export const approvedMessage = (l: Pick<Lesson, "kind" | "title">): string =>
  l.kind === "skill" ? `Lezione approvata: «${l.title}» è una nuova skill` : `Lezione approvata: «${l.title}» è in AGENTS.md`;

export type LessonEditCheck = { ok: true; title: string; content: string } | { ok: false; error: string };

/** The same checks as the engine, so the form says what is wrong before the request. */
export function checkLessonEdit(title: string, content: string): LessonEditCheck {
  const t = title.trim();
  const c = content.trim();
  if (!t) return { ok: false, error: "Il titolo non può essere vuoto." };
  if (!c) return { ok: false, error: "Il contenuto non può essere vuoto." };
  if ([...t].length > MAX_LESSON_TITLE_CHARS) return { ok: false, error: `Il titolo supera i ${MAX_LESSON_TITLE_CHARS} caratteri.` };
  if ([...c].length > MAX_LESSON_CONTENT_CHARS) return { ok: false, error: `Il contenuto supera i ${MAX_LESSON_CONTENT_CHARS} caratteri.` };
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
