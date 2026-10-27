import type { ReactNode } from "react";

/**
 * Renders the small Markdown subset AI answers use: headings, paragraphs,
 * lists, code blocks, `code` and **bold**. Builds React elements, never HTML,
 * so model output can't inject markup.
 */
export default function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim().startsWith("```")) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) code.push(lines[i++]);
      i++;
      blocks.push(
        <pre key={blocks.length} className="code">
          {code.join("\n")}
        </pre>,
      );
    } else if (/^#{1,6}\s/.test(line)) {
      blocks.push(
        <h4 key={blocks.length} className="md-heading">
          {inline(line.replace(/^#+\s*/, ""))}
        </h4>,
      );
      i++;
    } else if (/^\s*([-*]|\d+[.)])\s+/.test(line)) {
      const ordered = /^\s*\d/.test(line);
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+[.)])\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*([-*]|\d+[.)])\s+/, ""));
        i++;
      }
      const children = items.map((item, n) => <li key={n}>{inline(item)}</li>);
      blocks.push(ordered ? <ol key={blocks.length}>{children}</ol> : <ul key={blocks.length}>{children}</ul>);
    } else if (line.trim() === "") {
      i++;
    } else {
      const para: string[] = [];
      while (
        i < lines.length &&
        lines[i].trim() !== "" &&
        !lines[i].trim().startsWith("```") &&
        !/^#{1,6}\s/.test(lines[i]) &&
        !/^\s*([-*]|\d+[.)])\s+/.test(lines[i])
      ) {
        para.push(lines[i++]);
      }
      blocks.push(<p key={blocks.length}>{inline(para.join(" "))}</p>);
    }
  }
  return <div className="markdown">{blocks}</div>;
}

/** `code` and **bold** inside a line. */
function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) parts.push(text.slice(last, index));
    const token = match[0];
    parts.push(
      token.startsWith("`") ? (
        <code key={index}>{token.slice(1, -1)}</code>
      ) : (
        <strong key={index}>{token.slice(2, -2)}</strong>
      ),
    );
    last = index + token.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}
