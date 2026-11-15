import { useEffect, useId, useRef, useState } from "react";
import { copyText } from "../lib/clipboard";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { grammarFor } from "./highlight/languages";
import { useHighlight } from "./highlight/useHighlight";

import { t, t as tr } from "../i18n";
/** Copy button with a short "Copiato" confirmation. */
export function CopyButton({ text, label = tr("ui.copia"), className }: { text: string; label?: string; className?: string }) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);
  const copy = async () => {
    if (await copyText(text)) {
      setDone(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setDone(false), 1600);
    }
  };
  return (
    <button type="button" className={cx("btn sm", className)} onClick={() => void copy()} aria-live="polite">
      <Icon name={done ? "check" : "copy"} size={14} />
      {done ? t("ui.copiato") : label}
    </button>
  );
}

/**
 * Code or command snippet with a copy button. `lang` (the language of a Markdown fence, or given by the caller) colours it
 * when there is a grammar for it; the grammars load on demand, and the text is the same plain or coloured (what is copied and
 * what a screen reader reads never changes).
 */
export function CodeBlock({ code, title, lang }: { code: string; title?: string; lang?: string }) {
  const tokens = useHighlight(code, lang);
  const grammar = grammarFor(lang);
  return (
    <div className="codeblock">
      <div className="codeblock-head">
        <span className="codeblock-title">{title}</span>
        <CopyButton text={code} />
      </div>
      <pre tabIndex={0}>
        <code className={grammar ? `language-${grammar}` : undefined}>
          {tokens ? tokens.map((tk, i) => (tk.k ? <span key={i} className={`tok-${tk.k}`}>{tk.v}</span> : tk.v)) : code}
        </code>
      </pre>
    </div>
  );
}

/** Read-only value (URL, token, id) with a copy button; `secret` masks it until revealed. */
export function CopyField({ label, value, secret = false, hint }: { label: string; value: string; secret?: boolean; hint?: string }) {
  const [shown, setShown] = useState(!secret);
  const id = useId();
  return (
    <div className="field copyfield">
      <label htmlFor={id}>{label}</label>
      <div className="copyfield-row">
        <input id={id} className="input mono" readOnly value={shown ? value : "•".repeat(Math.min(24, value.length))} onFocus={(e) => shown && e.currentTarget.select()} />
        {secret && <button type="button" className="btn sm" onClick={() => setShown((v) => !v)}>{shown ? t("ui.nascondi") : t("ui.mostra")}</button>}
        <CopyButton text={value} />
      </div>
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}
