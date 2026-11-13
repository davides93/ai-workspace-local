// English messages of "lib". One entry per line; must have exactly the keys of the Italian file.
import type { libIt } from "../it/lib";

export const libEn: Record<keyof typeof libIt, string> = {
  "lib.kill_switch_attivato_per_run": "Kill switch engaged for runs, training, managed runtimes and the browser",
  "lib.kill_switch_non_riuscito": "Kill switch failed: {error}",
};
