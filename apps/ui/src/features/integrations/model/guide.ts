import { t } from "../../../shared/i18n";
import { TOKEN_PLACEHOLDER } from "./snippets";

/**
 * Configurations for the programs of others that can use AI Workspace as their model gateway (`/v1`) and as the
 * place of their agents (`/agui/{agent}`). Each one is read from the README and the guide of the project (see
 * `docs/agui.md`): nothing here was run against the real product, which needs a CopilotKit account.
 */
export interface ClientGuide {
  id: "openbot" | "opendots";
  /** Tab label: the name of the project. */
  title: string;
  /** What the configuration does, and what it does not cover. */
  note: string;
  /** The `.env` lines (and the steps in the app) to copy, with comments. */
  code: string;
}

const strip = (s: string) => s.replace(/\/+$/, "");
/** `host[:port]` of a base URL, the form `AGENT_ENDPOINT_ALLOWED_HOSTS` wants (an entry written as a URL stops OpenBot at startup). */
const hostOf = (base: string) => base.replace(/^[a-z]+:\/\//i, "");

export function buildClientGuides(baseUrl: string, opts: { token?: boolean } = {}): ClientGuide[] {
  const base = strip(baseUrl);
  const key = opts.token ? TOKEN_PLACEHOLDER : "locale";
  const bearer = `Bearer ${TOKEN_PLACEHOLDER}`;
  return [
    {
      id: "openbot",
      title: "OpenBot",
      note: t("integrations.guida_openbot_nota"),
      code: `# ${t("integrations.guida_openbot_modello")}
OPENAI_BASE_URL=${base}/v1
OPENAI_API_KEY=${key}
COPILOTKIT_OPENAI_API=chat

# ${t("integrations.guida_openbot_rete")}
AGENT_ENDPOINT_ALLOWED_HOSTS=${hostOf(base)}

# ${t("integrations.guida_openbot_agente")}
#   ${t("integrations.guida_openbot_endpoint")}: ${base}/agui/assistant${opts.token ? `\n#   ${t("integrations.guida_openbot_header")}: Authorization: ${bearer}` : ""}`,
    },
    {
      id: "opendots",
      title: "OpenDots",
      note: t("integrations.guida_opendots_nota"),
      code: `# ${t("integrations.guida_opendots_env")}
OPENAI_BASE_URL=${base}/v1
OPENAI_API_KEY=${key}
OPENAI_MODEL=balanced`,
    },
  ];
}
