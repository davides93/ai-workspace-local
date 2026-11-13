import { errorStatus } from "../../../shared/lib/errors";

import { t } from "../../../shared/i18n";
/**
 * The failure is about credentials: HTTP 401 (token missing or refused by the provider) or 403, or
 * a download job whose message says the repository is gated. Offers the "Imposta token" shortcut.
 */
export function needsToken(error: unknown, message?: string): boolean {
  const status = errorStatus(error);
  if (status === 401 || status === 403) return true;
  const text = message ?? (error instanceof Error ? error.message : typeof error === "string" ? error : "");
  return /gated|\btoken\b/i.test(text);
}

/**
 * Retry rule of the provider queries (search, files of a model): one more attempt on a network or
 * server error, none on a 4xx (a missing token, a wrong name or a refusal will not change by asking again).
 */
export function shouldRetryHub(failureCount: number, error: unknown): boolean {
  const status = errorStatus(error);
  if (status !== undefined && status >= 400 && status < 500) return false;
  return failureCount < 1;
}

/** Heading of a block-level error, by what went wrong. */
export function errorTitle(error: unknown): string {
  if (needsToken(error)) return t("models.accesso_negato_dal_provider");
  switch (errorStatus(error)) {
    case 404:
      return t("models.non_trovato");
    case 502:
      return t("models.provider_irraggiungibile");
    case 507:
      return t("models.spazio_su_disco_insufficiente");
    default:
      return t("models.qualcosa_non_ha_funzionato");
  }
}

/** Next step for the HTTP statuses of the hub (docs/architecture/model-hub.md §9); null when the backend message says enough. */
export function errorHint(error: unknown): string | null {
  switch (errorStatus(error)) {
    case 401:
    case 403:
      return t("models.imposta_il_token_del_provider");
    case 404:
      return t("models.controlla_il_nome_esatto_del");
    case 409:
      return t("models.lo_stesso_file_e_gia");
    case 502:
      return t("models.il_provider_non_risponde_controlla");
    case 507:
      return t("models.non_c_e_abbastanza_spazio");
    default:
      return null;
  }
}
