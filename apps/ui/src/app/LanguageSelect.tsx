import { useLanguage } from "./useLanguage";
import { Segmented } from "../shared/ui";
import { LANGUAGE_NAME, t } from "../shared/i18n";

/** The language choice (`ui.language`): automatic, Italiano, English. Used in Settings, the "Altro" sheet and the first-run screen. */
export function LanguageSelect({ label }: { label?: string }) {
  const [language, setLanguage] = useLanguage();
  return (
    <Segmented
      label={label ?? t("common.language")}
      value={language}
      onChange={setLanguage}
      options={[
        { id: "auto", label: t("common.language_auto_short") },
        { id: "it", label: LANGUAGE_NAME.it },
        { id: "en", label: LANGUAGE_NAME.en },
      ]}
    />
  );
}
