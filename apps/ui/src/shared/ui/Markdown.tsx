import { createElement, Fragment, memo, useMemo, type ReactNode } from "react";
import { cx } from "../lib/cx";
import { CodeBlock } from "./CodeBlock";
import { parseMarkdown, type Block, type Inline, type ListItem } from "./markdownParse";
import "./markdown.css";

function renderInline(nodes: Inline[]): ReactNode[] {
  return nodes.map((n, i) => {
    switch (n.t) {
      case "text":
        return <Fragment key={i}>{n.v}</Fragment>;
      case "code":
        return <code key={i}>{n.v}</code>;
      case "strong":
        return <strong key={i}>{renderInline(n.c)}</strong>;
      case "em":
        return <em key={i}>{renderInline(n.c)}</em>;
      case "del":
        return <del key={i}>{renderInline(n.c)}</del>;
      case "br":
        return <br key={i} />;
      case "link":
        return <a key={i} href={n.href} target="_blank" rel="noopener noreferrer">{renderInline(n.c)}</a>;
    }
  });
}

function ListItemView({ item, tight }: { item: ListItem; tight: boolean }) {
  const content = item.blocks.map((b, i) =>
    tight && b.t === "p" ? <Fragment key={i}>{renderInline(b.c)}</Fragment> : <BlockView key={i} block={b} />,
  );
  if (item.checked === undefined) return <li>{content}</li>;
  return (
    <li className="md-task">
      <input type="checkbox" checked={item.checked} disabled aria-label={item.checked ? "Completato" : "Da fare"} />
      <div>{content}</div>
    </li>
  );
}

/** One top-level block. Memoized on the block object, which the parse cache keeps stable while a message streams. */
const BlockView = memo(function BlockView({ block }: { block: Block }) {
  switch (block.t) {
    case "p":
      return <p>{renderInline(block.c)}</p>;
    case "h":
      // Messages sit under the page's h1/h2: their headings start at h3.
      return createElement(`h${block.level + 2}`, { className: `md-h md-h${block.level}` }, renderInline(block.c));
    case "code":
      return <CodeBlock code={block.v} title={block.lang || undefined} />;
    case "list": {
      const items = block.items.map((it, i) => <ListItemView key={i} item={it} tight={block.tight} />);
      return block.ordered ? <ol start={block.start}>{items}</ol> : <ul>{items}</ul>;
    }
    case "quote":
      return <blockquote>{block.c.map((b, i) => <BlockView key={i} block={b} />)}</blockquote>;
    case "hr":
      return <hr />;
    case "table":
      return (
        <div className="md-table">
          <table>
            <thead>
              <tr>{block.head.map((h, i) => <th key={i} scope="col" className={block.align[i] ? `md-al-${block.align[i]}` : undefined}>{renderInline(h)}</th>)}</tr>
            </thead>
            <tbody>
              {block.rows.map((r, ri) => (
                <tr key={ri}>{r.map((c, i) => <td key={i} className={block.align[i] ? `md-al-${block.align[i]}` : undefined}>{renderInline(c)}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
});

/**
 * Safe Markdown (no HTML passthrough, see `markdownParse.ts`). Parsing is cached per text and
 * incremental, so a message that streams re-parses only its last block and the older messages
 * of the conversation cost nothing.
 */
export const Markdown = memo(function Markdown({ text, streaming = false, className }: { text: string; streaming?: boolean; className?: string }) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return (
    <div className={cx("md", streaming && "is-streaming", className)}>
      {blocks.map((b, i) => <BlockView key={i} block={b} />)}
    </div>
  );
});
