import { useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Icon, ModalShell } from "../../shared/ui";
import { filterCommands, type Command } from "./commands";

import { t } from "../../shared/i18n";
/** Ctrl/Cmd+K palette: jump to any section or run a global action without leaving the keyboard. */
const groupName = (g: "go" | "actions") => t(g === "go" ? "shell.palette_go" : "shell.palette_actions");

export function CommandPalette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);


  const results = useMemo(() => filterCommands(commands, query), [commands, query]);
  const index = Math.min(active, Math.max(0, results.length - 1));
  const optionId = (i: number) => `${listId}-opt-${i}`;

  const run = (c: Command) => {
    onClose();
    c.run();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = results.length === 0 ? 0 : (index + (e.key === "ArrowDown" ? 1 : -1) + results.length) % results.length;
      setActive(next);
      listRef.current?.querySelector(`#${CSS.escape(optionId(next))}`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (results[index]) run(results[index]);
    }
  };

  const groups = (["go", "actions"] as const).map((g) => ({ g, items: results.filter((c) => c.group === g) })).filter((x) => x.items.length > 0);
  let flat = -1;

  return (
    <ModalShell variant="palette" title={t("shell.cerca_e_comandi")} onClose={onClose}>
      <div className="palette-input">
        <Icon name="search" size={18} />
        <input
          data-autofocus
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={results.length ? optionId(index) : undefined}
          aria-autocomplete="list"
          aria-label={t("shell.cerca_una_sezione_o_un")}
          placeholder={t("shell.cerca_una_sezione_o_un_2")}
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => { setQuery(e.target.value); setActive(0); }}
          onKeyDown={onKey}
        />
        <kbd>Esc</kbd>
      </div>
      <div id={listId} role="listbox" aria-label={t("shell.risultati")} className="palette-list" ref={listRef}>
        {groups.map(({ g, items }) => (
          <div key={g} role="group" aria-label={groupName(g)}>
            <div className="palette-group eyebrow" aria-hidden="true">{groupName(g)}</div>
            {items.map((c) => {
              flat += 1;
              const i = flat;
              return (
                <div key={c.id} id={optionId(i)} role="option" aria-selected={i === index} className="palette-item" onMouseMove={() => setActive(i)} onClick={() => run(c)}>
                  <Icon name={c.icon} size={16} />
                  <span className="grow">{c.label}</span>
                  {c.hint && <span className="faint text-xs">{c.hint}</span>}
                </div>
              );
            })}
          </div>
        ))}
        {results.length === 0 && <div className="palette-empty muted">{t("shell.nessun_risultato_per", { query })}</div>}
      </div>
    </ModalShell>
  );
}
