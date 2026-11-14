import { createContext, useContext } from "react";
import type { NoticeAction } from "./model/runNotice";

/**
 * What a note of the conversation can do in the simple interface (see `NoticeAction`): the chat provides it to the rows,
 * which would otherwise need it passed through every layer. `null` outside the chat.
 */
export interface ChatActions {
  /** Puts the sentence in the message box (below what is typed) and focuses it. */
  askAssistant: (text: string) => void;
  /** Switches to developer mode and opens `to`. */
  showAllSections: (to: string) => void;
}

export const ChatActionsContext = createContext<ChatActions | null>(null);

export const useChatActions = (): ChatActions | null => useContext(ChatActionsContext);

/** Runs a note's action with the chat's actions. */
export const runNoticeAction = (a: NoticeAction, actions: ChatActions): void => (a.kind === "ask" ? actions.askAssistant(a.text) : actions.showAllSections(a.to));
