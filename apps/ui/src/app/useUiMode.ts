import { useCallback, useEffect } from "react";
import { useConfig, useSetConfig } from "../features/settings/api";
import { configUiMode, uiModeLayer } from "../features/settings/model/config";
import { setStoredUiMode, useStoredUiMode, type UiMode } from "../shared/lib/uiMode";
import { confirmedUiMode } from "./routeGate";

import { t } from "../shared/i18n";
/**
 * The interface mode, a setter and what the configuration says. The configuration (`ui.mode`) decides;
 * the last known value is kept on the device so the shell does not flash the wrong sections while the
 * configuration loads. The third element is the mode the configuration confirms (`confirmedUiMode`): what it
 * reported, `undefined` until it has answered, and the value kept on the device only if it cannot be read at
 * all. The shell redirects only on that (`routeGate`), never on the value kept on the device while the
 * configuration is still on its way.
 */
export function useUiMode(): [UiMode, (mode: UiMode) => void, UiMode | undefined] {
  const mode = useStoredUiMode();
  const cfg = useConfig();
  const setConfig = useSetConfig();
  const fromConfig = configUiMode(cfg.data);

  // Follow the configuration when it says something else (another device changed it, or the
  // value was edited in the Impostazioni list).
  useEffect(() => {
    if (fromConfig) setStoredUiMode(fromConfig);
  }, [fromConfig]);

  const view = cfg.data;
  const set = useCallback(
    (next: UiMode) => {
      const before = configUiMode(view) ?? mode;
      setStoredUiMode(next);
      setConfig.mutate(
        { layer: uiModeLayer(view), key: "ui.mode", value: next, title: t("shell.modalita_dell_interfaccia") },
        { onError: () => setStoredUiMode(before) },
      );
    },
    [view, mode, setConfig],
  );
  return [mode, set, confirmedUiMode({ fromConfig, status: cfg.status, error: cfg.error, stored: mode })];
}
