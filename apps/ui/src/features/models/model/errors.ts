import { errorStatus } from "../../../shared/lib/errors";

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
  if (needsToken(error)) return "Accesso negato dal provider";
  switch (errorStatus(error)) {
    case 404:
      return "Non trovato";
    case 502:
      return "Provider irraggiungibile";
    case 507:
      return "Spazio su disco insufficiente";
    default:
      return "Qualcosa non ha funzionato";
  }
}

/** Next step for the HTTP statuses of the hub (docs/architecture/model-hub.md §9); null when the backend message says enough. */
export function errorHint(error: unknown): string | null {
  switch (errorStatus(error)) {
    case 401:
    case 403:
      return "Imposta il token del provider e riprova.";
    case 404:
      return "Controlla il nome esatto del modello o del file.";
    case 409:
      return "Lo stesso file è già in scrittura da un altro download: aspetta che finisca.";
    case 502:
      return "Il provider non risponde: controlla la connessione o l'endpoint (mirror) nelle impostazioni del provider, poi riprova.";
    case 507:
      return "Non c'è abbastanza spazio su disco: libera spazio o elimina un modello installato.";
    default:
      return null;
  }
}
