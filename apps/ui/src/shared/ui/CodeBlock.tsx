import { useEffect, useId, useRef, useState } from "react";
import { copyText } from "../lib/clipboard";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";

/** Copy button with a short "Copiato" confirmation. */
export function CopyButton({ text, label = "Copia", className }: { text: string; label?: string; className?: string }) {
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
      {done ? "Copiato" : label}
    </button>
  );
}

/** Code or command snippet with a copy button. */
export function CodeBlock({ code, title }: { code: string; title?: string }) {
  return (
    <div className="codeblock">
      <div className="codeblock-head">
        <span className="codeblock-title">{title}</span>
        <CopyButton text={code} />
      </div>
      <pre tabIndex={0}><code>{code}</code></pre>
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
        {secret && <button type="button" className="btn sm" onClick={() => setShown((v) => !v)}>{shown ? "Nascondi" : "Mostra"}</button>}
        <CopyButton text={value} />
      </div>
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}
