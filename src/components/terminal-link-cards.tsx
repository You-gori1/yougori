import { memo, useCallback, useMemo, useState } from "react"
import { CheckIcon, CopyIcon, QrCodeIcon, XIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { terminalClipboard } from "@/lib/terminal-clipboard"
import { isLocalTerminalLink } from "@/lib/terminal-links"
import { createTerminalQr } from "@/lib/terminal-qr"

const LinkCard = memo(function LinkCard({ url, onDismiss }: { url: string; onDismiss(url: string): void }) {
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState("")
  const qr = useMemo(() => createTerminalQr(url), [url])
  return <article className="relative flex shrink-0 items-center gap-3 rounded-lg border bg-background p-2 pr-8" data-terminal-link={url}>
    <Button type="button" size="icon-xs" variant="ghost" className="absolute top-1 right-1 text-muted-foreground" aria-label={`Dismiss QR code for ${url}`} title="Remove QR code" onClick={() => onDismiss(url)}><XIcon aria-hidden="true" className="size-3" /></Button>
    {qr ? <svg aria-label={`QR code for ${url}`} role="img" viewBox={`0 0 ${qr.size} ${qr.size}`} width="112" height="112" className="shrink-0 rounded bg-white" shapeRendering="crispEdges"><rect width={qr.size} height={qr.size} fill="white" /><path d={qr.path} fill="black" /></svg> : <p className="w-28 text-xs text-muted-foreground">This link is too long for a QR code.</p>}
    <div className="flex w-44 min-w-0 flex-col gap-2 font-sans">
      <p className="line-clamp-2 break-all text-xs text-foreground" title={url}>{url}</p>
      {isLocalTerminalLink(url) ? <p className="text-[10px] leading-relaxed text-muted-foreground">Localhost is device-only. Connect this service to Local network or Public access to use it on a phone.</p> : null}
      <Button type="button" size="xs" variant="ghost" className="self-start" aria-label={`Copy link ${url}`} onClick={() => { setError(""); void terminalClipboard.writeText(url).then(() => setCopied(true)).catch(() => setError("Couldn't copy. Try selecting the link text.")) }}>{copied ? <CheckIcon aria-hidden="true" /> : <CopyIcon aria-hidden="true" />}{copied ? "Copied" : "Copy link"}</Button>
      {error ? <p role="alert" className="text-[10px] text-destructive-foreground">{error}</p> : null}
    </div>
  </article>
})

export function TerminalLinkCards({ links }: { links: string[] }) {
  // This component stays mounted even when every card is hidden, so terminal
  // rescans and tab switches cannot immediately resurrect dismissed URLs.
  const [dismissed, setDismissed] = useState<Set<string>>(() => new Set())
  const dismiss = useCallback((url: string) => setDismissed(previous => new Set(previous).add(url)), [])
  const visibleLinks = links.filter(url => !dismissed.has(url))
  if (!visibleLinks.length) return null
  return <section aria-label="Terminal link QR codes" className="shrink-0 border-t bg-muted/30 text-foreground">
    <div className="flex items-center justify-between gap-2 px-3 pt-2 font-sans text-[10px] text-muted-foreground">
      <span className="flex items-center gap-1.5"><QrCodeIcon aria-hidden="true" className="size-3 shrink-0" />Scan a link · generated on this PC · latest 8 links</span>
      <Button type="button" size="xs" variant="ghost" className="shrink-0" onClick={() => setDismissed(previous => new Set([...previous, ...links]))}><XIcon aria-hidden="true" className="size-3" />Close all</Button>
    </div>
    <div className="flex gap-2 overflow-x-auto p-2 [scrollbar-width:thin]">{visibleLinks.map(url => <LinkCard key={url} url={url} onDismiss={dismiss} />)}</div>
  </section>
}
