import type { ReactNode } from "react";

/** Minimal, safe Markdown (no HTML passthrough). */
export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) buf.push(lines[i++]);
      i++;
      blocks.push(<pre key={key++}><code>{buf.join("\n")}</code></pre>);
      continue;
    }
    if (/^\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\|[\s:|-]+\|\s*$/.test(lines[i + 1])) {
      const rows: string[][] = [];
      const cells = (l: string) => l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const head = cells(line);
      i += 2;
      while (i < lines.length && /^\|.*\|\s*$/.test(lines[i])) rows.push(cells(lines[i++]));
      blocks.push(
        <table key={key++}>
          <thead><tr>{head.map((h, j) => <th key={j}>{inline(h)}</th>)}</tr></thead>
          <tbody>{rows.map((r, ri) => <tr key={ri}>{r.map((c, j) => <td key={j}>{inline(c)}</td>)}</tr>)}</tbody>
        </table>,
      );
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*]|\d+\.)\s+/, ""));
      const children = items.map((it, j) => <li key={j}>{inline(it)}</li>);
      blocks.push(ordered ? <ol key={key++}>{children}</ol> : <ul key={key++}>{children}</ul>);
      continue;
    }
    if (/^#{1,3}\s/.test(line)) {
      blocks.push(<h3 key={key++}>{inline(line.replace(/^#{1,3}\s/, ""))}</h3>);
      i++;
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !lines[i].startsWith("```") && !/^\|.*\|\s*$/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) para.push(lines[i++]);
    blocks.push(<p key={key++}>{inline(para.join(" "))}</p>);
  }
  return <div className="md">{blocks}</div>;
}

function inline(s: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(s))) {
    if (m.index > last) out.push(s.slice(last, m.index));
    const t = m[0];
    if (t.startsWith("`")) out.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else if (t.startsWith("**")) out.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    else {
      const mm = /^\[([^\]]+)\]\((.+)\)$/.exec(t)!;
      out.push(<a key={k++} href={mm[2]} target="_blank" rel="noreferrer">{mm[1]}</a>);
    }
    last = m.index + t.length;
  }
  if (last < s.length) out.push(s.slice(last));
  return out;
}
