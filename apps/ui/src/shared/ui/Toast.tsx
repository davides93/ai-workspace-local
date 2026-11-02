import { dismissToast, useToasts } from "../lib/toast";

/**
 * Renders the toast queue. Normal messages sit in a polite live region, failures in an
 * assertive one (`role="alert"`) so screen readers announce them at once.
 */
export function ToastHost() {
  const toasts = useToasts();
  return (
    <div className="toasts">
      <div role="status" aria-live="polite">
        {toasts.filter((t) => !t.tone).map((t) => (
          <div key={t.id} className="toast" onClick={() => dismissToast(t.id)}>{t.text}</div>
        ))}
      </div>
      <div role="alert">
        {toasts.filter((t) => t.tone).map((t) => (
          <div key={t.id} className={`toast ${t.tone}`} onClick={() => dismissToast(t.id)}>{t.text}</div>
        ))}
      </div>
    </div>
  );
}
