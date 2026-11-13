import { useEffect, useRef, useState } from "react";
import { copyText } from "../../../shared/lib/clipboard";
import { IconButton } from "../../../shared/ui";

import { t } from "../../../shared/i18n";
/** Copies the Markdown source of a reply; the icon turns into a check for a moment. */
export function CopyMessage({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);
  return (
    <IconButton
      icon={done ? "check" : "copy"}
      size={15}
      aria-label={done ? t("chat.risposta_copiata") : t("chat.copia_la_risposta")}
      title={done ? t("chat.copiata") : t("chat.copia_la_risposta")}
      onClick={async () => {
        if (!(await copyText(text))) return;
        setDone(true);
        if (timer.current) clearTimeout(timer.current);
        timer.current = setTimeout(() => setDone(false), 1600);
      }}
    />
  );
}
