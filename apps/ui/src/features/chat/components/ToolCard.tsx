import { memo, useId, useState } from "react";
import { cx } from "../../../shared/lib/cx";
import { prettyJson, summarizeArgs } from "../../../shared/lib/args";
import { CopyButton, Icon, Led, Pill } from "../../../shared/ui";
import type { ToolItem } from "../model/items";
import { resultPreview, STATUS_INFO, toolCategory, toolDuration, toolTitle } from "../model/tools";
import { ImageThumb } from "./ImageThumb";

const CLAMP_LINES = 16;
const CLAMP_CHARS = 1600;

/** Text block of a tool card (arguments, result) with a copy button; long text is cut until asked for. */
function ToolSection({ label, text, tone }: { label: string; text: string; tone?: "crit" }) {
  const [all, setAll] = useState(false);
  const lines = text.split("\n");
  const long = lines.length > CLAMP_LINES || text.length > CLAMP_CHARS;
  const shown = all || !long ? text : lines.slice(0, CLAMP_LINES).join("\n").slice(0, CLAMP_CHARS);
  return (
    <div className="codeblock ws-section">
      <div className="codeblock-head">
        <span className="codeblock-title">{label}</span>
        <CopyButton text={text} className="ghost" />
      </div>
      <pre tabIndex={0} className={cx(tone === "crit" && "is-crit")}><code>{shown}{long && !all ? "\n…" : ""}</code></pre>
      {long && (
        <button type="button" className="ws-section-more" onClick={() => setAll((v) => !v)}>
          {all ? "Mostra meno" : `Mostra tutto (${lines.length} righe)`}
        </button>
      )}
    </div>
  );
}

function StatusMark({ tool }: { tool: ToolItem }) {
  const info = STATUS_INFO[tool.status];
  switch (tool.status) {
    case "running":
      return <><Led tone="run" /><span className="sr-only">{info.label}</span></>;
    case "done":
      return <><Icon name="check" size={15} className="ws-ok" /><span className="sr-only">{info.label}</span></>;
    default:
      return <Pill tone={info.tone}>{info.label}</Pill>;
  }
}

/**
 * One tool call: category icon, name and arguments on one line, a one-line preview of the answer,
 * screenshots as thumbnails, and (on demand) the full arguments and result with copy buttons.
 */
export const ToolCard = memo(function ToolCard({ tool }: { tool: ToolItem }) {
  const [open, setOpen] = useState(false);
  const bodyId = useId();
  const cat = toolCategory(tool.name);
  const { title, sub } = toolTitle(tool.name);
  const duration = toolDuration(tool.startedAt, tool.endedAt);
  const summary = summarizeArgs(tool.args, 200);
  const preview = tool.status === "running" ? "" : resultPreview(tool.result);
  const bad = tool.status === "failed" || tool.status === "denied";
  return (
    <div className={cx("ws-tool", `is-${tool.status}`)} data-category={cat.id}>
      <button type="button" className="ws-tool-head" aria-expanded={open} aria-controls={open ? bodyId : undefined} onClick={() => setOpen((v) => !v)}>
        <span className="ws-tool-icon" title={cat.label}>
          <Icon name={cat.icon} size={15} />
          <span className="sr-only">{cat.label}: </span>
        </span>
        <span className="ws-tool-main">
          <span className="ws-tool-line">
            <span className="ws-tool-name mono">{title}</span>
            {sub && <span className="ws-tool-server">{sub}</span>}
            {summary && <span className="ws-tool-args mono">{summary}</span>}
          </span>
          {preview && <span className={cx("ws-tool-preview", bad && "is-crit")}>{preview}</span>}
        </span>
        <span className="ws-tool-state">
          {duration && <span className="ws-tool-time num">{duration}</span>}
          <StatusMark tool={tool} />
          <Icon name="down" size={14} className="ws-tool-chev" />
        </span>
      </button>
      {tool.images && tool.images.length > 0 && (
        <div className="ws-tool-images">
          {tool.images.map((img, i) => (
            <ImageThumb key={i} image={img} label={`Schermata prodotta da ${title}${tool.images!.length > 1 ? ` (${i + 1} di ${tool.images!.length})` : ""}`} />
          ))}
        </div>
      )}
      {open && (
        <div id={bodyId} className="ws-tool-body">
          {tool.args.trim() !== "" && tool.args.trim() !== "{}" && <ToolSection label="Argomenti" text={prettyJson(tool.args)} />}
          {tool.result !== undefined ? (
            <ToolSection label="Risultato" text={tool.result} tone={bad ? "crit" : undefined} />
          ) : (
            <p className="ws-tool-none">{tool.status === "running" ? "In attesa del risultato…" : "Nessun risultato: l'esecuzione si è interrotta prima."}</p>
          )}
        </div>
      )}
    </div>
  );
});
