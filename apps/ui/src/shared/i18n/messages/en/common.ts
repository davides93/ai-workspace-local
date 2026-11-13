// English messages of "common". One entry per line; must have exactly the keys of the Italian file.
import type { commonIt } from "../it/common";

export const commonEn: Record<keyof typeof commonIt, string> = {
  "time.now": "now",
  "time.days": "{n} d",
  "common.language": "Language",
  "common.language_auto": "Automatic (device language)",
  "common.language_auto_short": "Automatic",
  "common.language_hint": "Automatic follows the language of your device. It also sets the language of the assistants' replies in new conversations.",
  "common.error_unknown": "Unknown error",
  "common.error_network": "Cannot reach the workspace: check the connection.",
};
