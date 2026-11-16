import type { ChannelInfo, ContextInfo, ContextMember } from "../../../shared/api/types";

/** The accounts of a channel that can be linked: the people on its list (a group id is not a person). */
export function linkableSenders(c: Pick<ChannelInfo, "kind" | "allowed">): string[] {
  return c.kind === "telegram" ? c.allowed.filter((a) => Number(a) > 0) : c.allowed;
}

/** Channels that have someone to link. */
export const linkableChannels = (channels: ChannelInfo[]): ChannelInfo[] => channels.filter((c) => linkableSenders(c).length > 0);

/** The members the form picked (`""` means «do not link»), in the order of the channels. */
export function pickedMembers(channels: ChannelInfo[], picks: Record<string, string>): ContextMember[] {
  return channels.filter((c) => picks[c.id]).map((c) => ({ channel_id: c.id, sender: picks[c.id] }));
}

/** Accounts already in a context, so the form does not offer them again (the engine refuses them: one context per account). */
export function takenSenders(contexts: ContextInfo[]): Set<string> {
  return new Set(contexts.flatMap((c) => c.members.map((m) => `${m.channel_id}|${m.sender}`)));
}

export const memberKey = (m: ContextMember): string => `${m.channel_id}|${m.sender}`;
