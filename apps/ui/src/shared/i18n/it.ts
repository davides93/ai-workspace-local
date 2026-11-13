// Italian catalog: the source of truth. Keys are typed (`MessageKey`); `en.ts` must provide every one.
import { activityIt } from "./messages/it/activity";
import { agentsIt } from "./messages/it/agents";
import { approvalsIt } from "./messages/it/approvals";
import { capabilitiesIt } from "./messages/it/capabilities";
import { channelsIt } from "./messages/it/channels";
import { chatIt } from "./messages/it/chat";
import { commonIt } from "./messages/it/common";
import { devicesIt } from "./messages/it/devices";
import { documentsIt } from "./messages/it/documents";
import { evalsIt } from "./messages/it/evals";
import { homeIt } from "./messages/it/home";
import { inboxIt } from "./messages/it/inbox";
import { integrationsIt } from "./messages/it/integrations";
import { learningIt } from "./messages/it/learning";
import { mcpIt } from "./messages/it/mcp";
import { modelsIt } from "./messages/it/models";
import { navIt } from "./messages/it/nav";
import { pairingIt } from "./messages/it/pairing";
import { runtimesIt } from "./messages/it/runtimes";
import { securityIt } from "./messages/it/security";
import { settingsIt } from "./messages/it/settings";
import { shellIt } from "./messages/it/shell";
import { teachIt } from "./messages/it/teach";
import { teamsIt } from "./messages/it/teams";
import { trainingIt } from "./messages/it/training";
import { uiIt } from "./messages/it/ui";
import { workflowsIt } from "./messages/it/workflows";

export const it = {
  ...activityIt,
  ...agentsIt,
  ...approvalsIt,
  ...capabilitiesIt,
  ...channelsIt,
  ...chatIt,
  ...commonIt,
  ...devicesIt,
  ...documentsIt,
  ...evalsIt,
  ...homeIt,
  ...inboxIt,
  ...integrationsIt,
  ...learningIt,
  ...mcpIt,
  ...modelsIt,
  ...navIt,
  ...pairingIt,
  ...runtimesIt,
  ...securityIt,
  ...settingsIt,
  ...shellIt,
  ...teachIt,
  ...teamsIt,
  ...trainingIt,
  ...uiIt,
  ...workflowsIt,
} as const;

export type MessageKey = keyof typeof it;
