// English messages of "learning". One entry per line; must have exactly the keys of the Italian file.
import type { learningIt } from "../it/learning";

export const learningEn: Record<keyof typeof learningIt, string> = {
  "learning.titolo": "Title",
  "learning.contenuto_markdown": "Content (Markdown)",
  "learning.fino_a_caratteri": "Up to {MAX_LESSON_CONTENT_CHARS} characters.",
  "learning.finche_non_approvi_non_viene": "{p} Nothing is written until you approve.",
  "learning.approva_con_le_modifiche": "Approve with the changes",
  "learning.approva": "Approve",
  "learning.annulla_le_modifiche": "Discard the changes",
  "learning.modifica": "Edit",
  "learning.scarta": "Dismiss",
  "learning.vedi_la_conversazione": "See the conversation",
  "learning.lezioni_da_rivedere": "Lessons to review",
  "learning.nessuna_lezione_in_attesa_dopo": "No lessons waiting. After a thumbs down the agent can propose what to remember: you decide.",
  "learning.vuoi_che_lo_ricordi": "Do you want me to remember it?",
  "learning.conferma": "Confirm",
};
