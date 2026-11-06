import { MAX_LESSON_CONTENT_CHARS, MAX_LESSON_TITLE_CHARS } from "../types";
import type { Lesson, Session } from "../types";

// Demo data and rules of the lessons API, as the engine applies them (crates/aiw-engine/src/learning.rs).

const iso = (minAgo: number) => new Date(Date.now() - minAgo * 60_000).toISOString();

export const lessons: Lesson[] = [
  {
    id: "les_demo_skill", session_id: "ses_bug", run_id: "run_demo_bug", agent_id: "coder", kind: "skill", title: "Correggere un test del parser GGUF",
    content: "# Correggere un test del parser GGUF\n\n1. Esegui `cargo test -p aiw-models gguf` e leggi il primo errore.\n2. Confronta i byte attesi con l'header del file di prova.\n3. Correggi il parser, non il test, salvo che il formato sia cambiato.\n4. Rilancia l'intera suite del crate prima di chiudere.",
    reason: "Il run è fallito perché il test è stato modificato invece del parser.", status: "pending", created_at: iso(1400),
  },
  {
    id: "les_demo_memory", session_id: "ses_pdf", agent_id: "operator", kind: "memory", title: "Chiedi prima dove spostare i file",
    content: "Prima di riordinare una cartella chiedi all'utente la destinazione e proponi l'elenco dei file da spostare.",
    reason: "L'utente ha scartato lo spostamento dei PDF perché non era la cartella che voleva.", status: "approved", created_at: iso(4000), decided_at: iso(3900),
  },
];

/** The checks the engine makes on what the user edits before approving; the first problem is the message. */
export function lessonProblem(title: string, content: string): string | null {
  if (!title.trim() || !content.trim()) return "titolo e contenuto della lezione non possono essere vuoti";
  if ([...title].length > MAX_LESSON_TITLE_CHARS || [...content].length > MAX_LESSON_CONTENT_CHARS) {
    return `titolo (max ${MAX_LESSON_TITLE_CHARS}) o contenuto (max ${MAX_LESSON_CONTENT_CHARS} caratteri) troppo lunghi`;
  }
  return null;
}

/** What the agent would propose after reflecting on a session (the demo has no model to ask). */
export function demoLesson(session: Session, id: string): Lesson {
  return {
    id, session_id: session.id, agent_id: session.agent_id, kind: "memory", title: `Cosa ricordare da «${session.title}»`,
    content: "Prima di agire riassumi in una riga cosa intendi fare e conferma l'obiettivo con l'utente.",
    reason: "Una conversazione si è chiusa con un pollice giù: conviene chiarire l'obiettivo prima di iniziare.", status: "pending", created_at: new Date().toISOString(),
  };
}
