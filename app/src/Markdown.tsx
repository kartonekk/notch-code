import type { ReactNode } from "react";

/**
 * Just enough Markdown for Claude's answers: ``` fences, headings, bullet and
 * numbered lists, quotes, rules, **bold**, *italic*, ~~strike~~, `code` and
 * [links](url) (shown as text, the address on hover).
 */
export function Markdown({ text }: { text: string }) {
  const out: ReactNode[] = [];
  const lines = text.split("\n");
  let para: string[] = [];
  let list: { ordered: boolean; start: number; items: string[] } | null = null;
  let quote: string[] = [];

  const flush = () => {
    const k = out.length;
    if (para.length) out.push(<p key={k}>{inline(para.join("\n"))}</p>);
    else if (quote.length) out.push(<blockquote key={k}>{inline(quote.join("\n"))}</blockquote>);
    else if (list) {
      const items = list.items.map((it, i) => <li key={i}>{inline(it)}</li>);
      out.push(list.ordered ? <ol key={k} start={list.start}>{items}</ol> : <ul key={k}>{items}</ul>);
    }
    para = [];
    quote = [];
    list = null;
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    let m: RegExpMatchArray | null;
    if (/^\s*```/.test(line)) {
      flush();
      const code: string[] = [];
      while (++i < lines.length && !/^\s*```/.test(lines[i])) code.push(lines[i]);
      out.push(
        <pre key={out.length} className="code-block">
          {code.join("\n")}
        </pre>,
      );
    } else if (!line.trim()) {
      flush();
    } else if ((m = line.match(/^(#{1,6})\s+(.*)$/))) {
      flush();
      out.push(
        <div key={out.length} className={`md-h md-h${Math.min(m[1].length, 3)}`}>
          {inline(m[2].replace(/\s+#+\s*$/, ""))}
        </div>,
      );
    } else if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flush();
      out.push(<hr key={out.length} />);
    } else if ((m = line.match(/^\s*(?:([-*+])|(\d+)[.)])\s+(.*)$/))) {
      const ordered = !m[1];
      if (para.length || quote.length || (list && list.ordered !== ordered)) flush();
      list ??= { ordered, start: ordered ? Number(m[2]) : 1, items: [] };
      list.items.push(m[3]);
    } else if ((m = line.match(/^>\s?(.*)$/))) {
      if (!quote.length) flush();
      quote.push(m[1]);
    } else if (list && /^\s{2,}\S/.test(line)) {
      // a wrapped list item
      list.items[list.items.length - 1] += "\n" + line.trim();
    } else {
      if (!para.length) flush();
      para.push(line);
    }
  }
  flush();
  return <>{out}</>;
}

const INLINE = /(`[^`\n]+`|\*\*[^*\n]+\*\*|__[^_\n]+__|~~[^~\n]+~~|\*[^*\s][^*\n]*\*|\[[^\]\n]+\]\([^)\s]+\))/;

function inline(text: string): ReactNode[] {
  return text.split(INLINE).map((s, i) => {
    if (i % 2 === 0) return s;
    if (s.startsWith("`")) return <code key={i}>{s.slice(1, -1)}</code>;
    if (s.startsWith("**") || s.startsWith("__")) return <strong key={i}>{inline(s.slice(2, -2))}</strong>;
    if (s.startsWith("~~")) return <s key={i}>{inline(s.slice(2, -2))}</s>;
    if (s.startsWith("*")) return <em key={i}>{inline(s.slice(1, -1))}</em>;
    const [, label, url] = s.match(/^\[(.*)\]\((.*)\)$/)!;
    return (
      <span key={i} className="md-link" title={url}>
        {inline(label)}
      </span>
    );
  });
}
