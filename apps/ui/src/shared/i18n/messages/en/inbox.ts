// English messages of "inbox". One entry per line; must have exactly the keys of the Italian file.
import type { inboxIt } from "../it/inbox";

export const inboxEn: Record<keyof typeof inboxIt, string> = {
  "inbox.proposte": "Suggestions",
  "inbox.cose_che_l_assistente_ha": "Things the assistant noticed while you were away. It does nothing on its own: it only goes ahead if you approve.",
  "inbox.controlla_adesso": "Check now",
  "inbox.fatto": "Done",
  "inbox.nessuna_proposta_per_ora": "No suggestions for now",
  "inbox.accendi_l_assistente_proattivo_dalle": "Turn on the proactive assistant in the settings and it will start looking around.",
  "inbox.quando_l_assistente_nota_qualcosa": "When the assistant notices something useful you will find it here.",
  "inbox.da_guardare": "To look at",
  "inbox.fatte": "Done",
  "inbox.dal_tuo_assistente": "From your assistant",
  "inbox.1_proposta": "1 suggestion",
  "inbox.proposte_2": "{open} suggestions",
  "inbox.guarda_le_proposte": "See the suggestions",
  "inbox.1_proposta_in_attesa": "1 suggestion waiting",
  "inbox.proposte_in_attesa": "{n} suggestions waiting",
  "inbox.cosa_faccio": "What I do",
  "inbox.cosa_farei": "What I would do:",
  "inbox.approva": "Approve",
  "inbox.annulla_le_modifiche": "Discard the changes",
  "inbox.modifica": "Edit",
  "inbox.scarta": "Dismiss",
  "inbox.apri_la_chat": "Open the chat",
};
