// English catalog. A key missing here (or one that does not exist in it.ts) is a type error.
//
// Glossary (keep the terms consistent):
//   Assistente -> Assistant        Agente -> Agent            Insegna -> Teach
//   Proposte -> Suggestions        Panoramica -> Home         Workspace (chat) -> Chat
//   Modelli -> Models              Impostazioni -> Settings   Sicurezza -> Security
//   Documenti -> Documents         Canali -> Channels         Dispositivi -> Devices
//   Attività -> Activity           Conferma / approvazione -> Approval
//   Ricordi -> Memories            Lezione -> Lesson          Workflow, Team, Runtime, MCP: unchanged
import type { MessageKey } from "./it";
import { activityEn } from "./messages/en/activity";
import { agentsEn } from "./messages/en/agents";
import { approvalsEn } from "./messages/en/approvals";
import { capabilitiesEn } from "./messages/en/capabilities";
import { channelsEn } from "./messages/en/channels";
import { chatEn } from "./messages/en/chat";
import { commonEn } from "./messages/en/common";
import { devicesEn } from "./messages/en/devices";
import { documentsEn } from "./messages/en/documents";
import { evalsEn } from "./messages/en/evals";
import { homeEn } from "./messages/en/home";
import { inboxEn } from "./messages/en/inbox";
import { integrationsEn } from "./messages/en/integrations";
import { learningEn } from "./messages/en/learning";
import { mcpEn } from "./messages/en/mcp";
import { modelsEn } from "./messages/en/models";
import { navEn } from "./messages/en/nav";
import { pairingEn } from "./messages/en/pairing";
import { runtimesEn } from "./messages/en/runtimes";
import { securityEn } from "./messages/en/security";
import { settingsEn } from "./messages/en/settings";
import { shellEn } from "./messages/en/shell";
import { teachEn } from "./messages/en/teach";
import { teamsEn } from "./messages/en/teams";
import { trainingEn } from "./messages/en/training";
import { uiEn } from "./messages/en/ui";
import { workflowsEn } from "./messages/en/workflows";

export const en: Record<MessageKey, string> = {
  ...activityEn,
  ...agentsEn,
  ...approvalsEn,
  ...capabilitiesEn,
  ...channelsEn,
  ...chatEn,
  ...commonEn,
  ...devicesEn,
  ...documentsEn,
  ...evalsEn,
  ...homeEn,
  ...inboxEn,
  ...integrationsEn,
  ...learningEn,
  ...mcpEn,
  ...modelsEn,
  ...navEn,
  ...pairingEn,
  ...runtimesEn,
  ...securityEn,
  ...settingsEn,
  ...shellEn,
  ...teachEn,
  ...teamsEn,
  ...trainingEn,
  ...uiEn,
  ...workflowsEn,
};
