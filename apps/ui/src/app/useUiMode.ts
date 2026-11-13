import { useCallback, useEffect } from "react";
import { useConfig, useSetConfig } from "../features/settings/api";
import { configUiMode, uiModeLayer } from "../features/settings/model/config";
import { setStoredUiMode, useStoredUiMode, type UiMode } from "../shared/lib/uiMode";

import { t } from "../shared/i18n";
/**
 * The interface mode and a setter. The configuration (`ui.mode`) decides; the last known value is
 * kept on the device so the shell does not flash the wrong sections while the configuration loads.
 */
export function useUiMode(): [UiMode, (mode: UiMode) => void] {
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
  return [mode, set];
}
