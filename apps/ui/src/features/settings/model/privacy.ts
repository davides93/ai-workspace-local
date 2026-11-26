import { t } from "../../../shared/i18n";

/** What «Solo locale» changes, in the configuration (all of these exist already; nothing is enforced anywhere else). */
export const LOCAL_ONLY_ON: ReadonlyArray<readonly [key: string, value: unknown]> = [
  ["search.provider", "off"],
  ["update.auto_check", false],
  ["update.auto_download", false],
];

/** What switching it off gives back: the defaults of those settings (the level of the proactive assistant is left as it is). */
export const LOCAL_ONLY_OFF: ReadonlyArray<readonly [key: string, value: unknown]> = [
  ["search.provider", "auto"],
  ["update.auto_check", true],
];

type Effective = Record<string, unknown> | undefined;

/** The proactive assistant may read the web only at «act_readonly»: «Solo locale» lowers it to «propose». */
const readsTheWeb = (eff: Effective) => eff?.["proactive.level"] === "act_readonly";

/** True when the web search is off, the updates are not checked or downloaded by themselves and the proactive assistant does not read the web. */
export function isLocalOnly(eff: Effective): boolean {
  if (!eff) return false;
  return eff["search.provider"] === "off" && eff["update.auto_check"] === false && eff["update.auto_download"] !== true && !readsTheWeb(eff);
}

/** The writes that turn «Solo locale» on (`on`) or off, given the configuration as it is now. */
export function localOnlyWrites(on: boolean, eff: Effective): Array<readonly [string, unknown]> {
  if (!on) return [...LOCAL_ONLY_OFF];
  const writes = [...LOCAL_ONLY_ON];
  if (readsTheWeb(eff)) writes.push(["proactive.level", "propose"]);
  return writes;
}

export interface PrivacyLine { id: string; text: string; leaves: boolean }

/** One line per thing that can leave the device, as the configuration stands. `leaves` says whether something is sent out. */
export function privacyLines(eff: Effective): PrivacyLine[] {
  const provider = String(eff?.["search.provider"] ?? "auto");
  const search: PrivacyLine = provider === "off"
    ? { id: "search", text: t("settings.privacy_ricerca_spenta"), leaves: false }
    : { id: "search", text: t("settings.privacy_ricerca_attiva"), leaves: true };
  const updates: PrivacyLine = eff?.["update.auto_check"] === false
    ? { id: "updates", text: t("settings.privacy_aggiornamenti_spenti"), leaves: false }
    : { id: "updates", text: t("settings.privacy_aggiornamenti_attivi"), leaves: true };
  const web: PrivacyLine = readsTheWeb(eff)
    ? { id: "proactive", text: t("settings.privacy_proattivo_web"), leaves: true }
    : { id: "proactive", text: t("settings.privacy_proattivo_locale"), leaves: false };
  return [
    { id: "data", text: t("settings.privacy_dati_locali"), leaves: false },
    search, updates, web,
  ];
}
