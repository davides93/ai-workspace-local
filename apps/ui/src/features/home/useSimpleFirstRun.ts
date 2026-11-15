import { useSystem } from "../../shared/query/system";
import { simpleAgentId } from "../../shared/lib/agents";
import { useAgents, useProfiles } from "../agents/api";
import { useSessions } from "../chat/api";
import { useCatalog, useModels, useRecommend } from "../models/api";
import { useRuntimes } from "../runtimes/api";
import { useConfig } from "../settings/api";
import { defaultProfile, followsAdviser, simpleFirstRun, type SimpleFirstRun } from "./model/overview";

/**
 * What the first-run panel of the simple interface needs, for the Assistente (the agent of every new simple session): the
 * state (`undefined` until every answer is in), and the data the panel acts on. A recommendation that is still being asked
 * for the first time is `loading`; a refresh of it later (after a download) does not flip the state back, so the panel
 * does not flash in an empty conversation.
 */
export function useSimpleFirstRun() {
  const runtimes = useRuntimes();
  const models = useModels();
  const agents = useAgents();
  const profiles = useProfiles();
  const sessions = useSessions();
  const config = useConfig();
  const catalog = useCatalog();
  const recommend = useRecommend();
  const system = useSystem();
  const agentId = simpleAgentId(agents.data ?? [], config.data?.effective["agent.default"]);
  const auto = followsAdviser(agents.data ?? [], agentId);
  const profile = defaultProfile(agents.data ?? [], profiles.data ?? [], agentId);
  const ready = !!(runtimes.data && models.data && profiles.data && sessions.data && agents.data);
  // An adviser that cannot answer (older server) is `null`: the first run falls back to the old rule.
  const recommendation = recommend.isError ? null : recommend.data;
  const state: SimpleFirstRun | undefined = ready
    ? simpleFirstRun({ runtimes: runtimes.data!, models: models.data!, profile, catalog: catalog.data, recommendation, recommendationPending: recommend.isPending, followsAdviser: auto })
    : undefined;
  // Something it needs failed and nothing is on its way: the caller shows its usual empty state instead of waiting for ever.
  const failed = [runtimes, models, agents, profiles, sessions].some((q) => q.isError);
  return { state, models: models.data ?? [], runtimes: runtimes.data ?? [], profile, device: system.data?.device, followsAdviser: auto, failed };
}
