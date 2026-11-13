import type { ReactNode } from "react";
import { errorMessage } from "../../../shared/lib/errors";
import { Button, Icon } from "../../../shared/ui";
import { errorHint, errorTitle, needsToken } from "../model/errors";

import { t } from "../../../shared/i18n";
/**
 * Failure of a provider call (search, files of a model, start of a download): the backend's Italian
 * message, the next step for its HTTP status, "Riprova" and, for 401/403 or a gated repository,
 * the "Imposta token" shortcut that opens the provider dialog.
 */
export function HubError({ error, title, onRetry, retrying, onSetToken, children }: {
  error: unknown;
  title?: string;
  onRetry?: () => void;
  retrying?: boolean;
  onSetToken?: () => void;
  children?: ReactNode;
}) {
  const hint = errorHint(error);
  const token = onSetToken && needsToken(error);
  return (
    <div className="empty error-state" role="alert">
      <Icon name="alert" size={22} />
      <h3>{title ?? errorTitle(error)}</h3>
      <p className="muted">{errorMessage(error)}</p>
      {hint && <p className="muted text-sm">{hint}</p>}
      {children}
      {(onRetry || token) && (
        <div className="row">
          {token && <Button variant="primary" size="sm" icon="key" onClick={onSetToken}>{t("models.imposta_token")}</Button>}
          {onRetry && <Button size="sm" icon="refresh" onClick={onRetry} disabled={retrying}>{t("models.riprova")}</Button>}
        </div>
      )}
    </div>
  );
}

/** Compact version of the same message for the spot where an action failed (a variant row, a card, a job). */
export function InlineError({ error, message, onSetToken }: { error?: unknown; message?: string; onSetToken?: () => void }) {
  const text = message ?? errorMessage(error);
  const hint = error === undefined ? null : errorHint(error);
  const token = onSetToken && needsToken(error, text);
  return (
    <div className="mh-alert" role="alert">
      <Icon name="alert" size={15} />
      <div className="grow">
        <span>{text}</span>
        {hint && <span className="faint"> {hint}</span>}
      </div>
      {token && <Button size="sm" icon="key" onClick={onSetToken}>{t("models.imposta_token")}</Button>}
    </div>
  );
}
