import { useState, type ReactNode } from "react"
import { terminalClipboard } from "@/lib/terminal-clipboard"
import { parseMarkdown } from "@/lib/model-chat"

// Renders model replies as React elements only, so model output can never inject HTML.

const inlinePattern = /(`+)([\s\S]+?)\1|\*\*([\s\S]+?)\*\*|__([\s\S]+?)__|~~([\s\S]+?)~~|\*([^*\s][^*]*?)\*|(?<![\w])_([^_\s][^_]*?)_(?![\w])|\[([^\]]+)\]\((\S+?)\)/g

function renderInline(text: string, key = "i"): ReactNode[] {
  const nodes: ReactNode[] = []
  let last = 0
  let n = 0
  for (const match of text.matchAll(inlinePattern)) {
    if (match.index > last) nodes.push(text.slice(last, match.index))
    const k = `${key}-${n++}`
    if (match[2] !== undefined) nodes.push(<code key={k}>{match[2]}</code>)
    else if (match[3] !== undefined || match[4] !== undefined) nodes.push(<strong key={k}>{renderInline(match[3] ?? match[4]!, k)}</strong>)
    else if (match[5] !== undefined) nodes.push(<del key={k}>{renderInline(match[5], k)}</del>)
    else if (match[6] !== undefined || match[7] !== undefined) nodes.push(<em key={k}>{renderInline(match[6] ?? match[7]!, k)}</em>)
    else nodes.push(<span key={k} className="md-link" title={match[9]}>{renderInline(match[8]!, k)}</span>)
    last = match.index + match[0].length
  }
  if (last < text.length) nodes.push(text.slice(last))
  return nodes
}

function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const [copied, setCopied] = useState(false)
  return <div className="md-code">
    <div className="md-code-bar"><span>{lang || "code"}</span><button type="button" onClick={() => void terminalClipboard.writeText(text).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1500) }).catch(() => undefined)}>{copied ? "Copied" : "Copy"}</button></div>
    <pre><code>{text}</code></pre>
  </div>
}

export function Markdown({ text }: { text: string }) {
  return <div className="md">{parseMarkdown(text).map((block, i) => {
    const k = `b${i}`
    switch (block.kind) {
      case "code": return <CodeBlock key={k} lang={block.lang} text={block.text} />
      case "heading": { const Tag = `h${Math.min(block.level + 2, 6)}` as "h3"; return <Tag key={k}>{renderInline(block.text, k)}</Tag> }
      case "list": return block.ordered
        ? <ol key={k} start={block.start}>{block.items.map((item, j) => <li key={j}>{renderInline(item, `${k}-${j}`)}</li>)}</ol>
        : <ul key={k}>{block.items.map((item, j) => <li key={j}>{renderInline(item, `${k}-${j}`)}</li>)}</ul>
      case "quote": return <blockquote key={k}>{renderInline(block.text, k)}</blockquote>
      case "table": return <div key={k} className="md-table"><table><thead><tr>{block.head.map((cell, j) => <th key={j}>{renderInline(cell, `${k}-h${j}`)}</th>)}</tr></thead><tbody>{block.rows.map((row, r) => <tr key={r}>{row.map((cell, j) => <td key={j}>{renderInline(cell, `${k}-${r}-${j}`)}</td>)}</tr>)}</tbody></table></div>
      case "rule": return <hr key={k} />
      default: return <p key={k}>{renderInline(block.text, k)}</p>
    }
  })}</div>
}
