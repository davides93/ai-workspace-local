import { useCallback, useEffect } from "react";
import { useConfig, useSetConfig } from "../features/settings/api";
import { winningLayer } from "../features/settings/model/config";
import { isLanguageSetting, setLanguageSetting, t, useLanguageSetting, type LanguageSetting } from "../shared/i18n";

/**
 * The language setting (`ui.language`: `auto`, `it`, `en`) and a setter. Like the interface mode,
 * the configuration decides; the last known value is kept on the device so the first paint is
 * already in the right language. `auto` follows the browser / operating system language.
 */
export function useLanguage(): [LanguageSetting, (next: LanguageSetting) => void] {
  const stored = useLanguageSetting();
  const cfg = useConfig();
  const setConfig = useSetConfig();
  const view = cfg.data;
  const fromConfig = view?.effective["ui.language"];

  // Follow the configuration when it says something else (another device, or the Impostazioni list).
  useEffect(() => {
    if (isLanguageSetting(fromConfig)) setLanguageSetting(fromConfig);
  }, [fromConfig]);

  const set = useCallback(
    (next: LanguageSetting) => {
      const before = isLanguageSetting(fromConfig) ? fromConfig : stored;
      setLanguageSetting(next);
      const winner = view ? winningLayer(view, "ui.language") : "global";
      setConfig.mutate(
        { layer: winner === "defaults" ? "global" : winner, key: "ui.language", value: next, title: t("common.language") },
        { onError: () => setLanguageSetting(before) },
      );
    },
    [view, fromConfig, stored, setConfig],
  );
  return [stored, set];
}
