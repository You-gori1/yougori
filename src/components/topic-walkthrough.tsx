import { useEffect, useLayoutEffect, useRef, useState } from "react"
import { createPortal } from "react-dom"
import { Button } from "@/components/ui/button"
import { clipTourRect, placeTourCard, type TourRect } from "@/lib/tour-position"
import { instructionTargets } from "@/lib/topic-walkthrough"
import type { InstructionGuide } from "@/lib/instruction-guides"
import "./instructions-tour.css"

function visibleTarget(selector: string): HTMLElement | null {
  return [...document.querySelectorAll<HTMLElement>(selector)].find(element => element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden" && !element.closest('[hidden], [inert], [aria-hidden="true"]')) ?? null
}

export function TopicWalkthrough({ guide, onComplete, onExit }: { guide: InstructionGuide; onComplete(id: string): void; onExit(): void }) {
  const [step, setStep] = useState(0)
  const [layout, setLayout] = useState<{ rects: TourRect[]; width: number; height: number; cardHeight: number }>({ rects: [], width: window.innerWidth, height: window.innerHeight, cardHeight: 250 })
  const [targetAvailable, setTargetAvailable] = useState(false)
  const card = useRef<HTMLDivElement>(null)
  const previousFocus = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null)
  const selector = instructionTargets[guide.id]?.[step]

  useEffect(() => () => {
    const target = previousFocus.current?.isConnected ? previousFocus.current : document.querySelector<HTMLElement>('[data-tour="instructions"]')
    target?.focus({ preventScroll: true })
  }, [])
  useEffect(() => { card.current?.focus({ preventScroll: true }) }, [step])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.repeat || visibleTarget('[data-slot="dialog-popup"], [data-slot="menu-popup"], [data-slot="combobox-popup"]')) return
      event.preventDefault(); event.stopImmediatePropagation(); onExit()
    }
    document.addEventListener("keydown", key, true)
    return () => document.removeEventListener("keydown", key, true)
  }, [onExit])

  useLayoutEffect(() => {
    let frame = 0, scrolled: HTMLElement | null = null, disposed = false
    const refresh = () => {
      frame = 0
      if (disposed) return
      const target = selector ? visibleTarget(selector) : null
      const width = window.innerWidth, height = window.visualViewport?.height ?? window.innerHeight
      if (target && target !== scrolled) {
        scrolled = target
        const rect = target.getBoundingClientRect()
        if (rect.top < 70 || rect.bottom > height - 12) target.scrollIntoView({ block: rect.height > height / 2 ? "start" : "center", inline: "nearest", behavior: "instant" })
      }
      const rects = target ? [clipTourRect(target.getBoundingClientRect(), width, height)].filter(rect => rect.width > 1 && rect.height > 1) : []
      setTargetAvailable(Boolean(target))
      setLayout(current => {
        const next = { rects, width, height, cardHeight: card.current?.getBoundingClientRect().height ?? 250 }
        return JSON.stringify(current) === JSON.stringify(next) ? current : next
      })
    }
    const schedule = () => { if (!frame) frame = requestAnimationFrame(refresh) }
    const resized = () => { scrolled = null; schedule() }
    const observer = new MutationObserver(records => { if (records.some(record => !(record.target instanceof Element && record.target.closest('[data-topic-ui]')))) schedule() })
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["style", "class", "hidden", "aria-pressed", "aria-expanded"] })
    const resize = new ResizeObserver(schedule)
    if (card.current) resize.observe(card.current)
    document.addEventListener("scroll", schedule, true)
    window.addEventListener("resize", resized); window.visualViewport?.addEventListener("resize", resized)
    refresh()
    return () => { disposed = true; cancelAnimationFrame(frame); observer.disconnect(); resize.disconnect(); document.removeEventListener("scroll", schedule, true); window.removeEventListener("resize", resized); window.visualViewport?.removeEventListener("resize", resized) }
  }, [selector])

  const position = placeTourCard(layout.rects, layout, { width: 352, height: layout.cardHeight })
  const last = step === guide.steps.length - 1
  return createPortal(<div className="tour-layer" data-topic-ui data-instruction-topic={guide.id} data-instruction-step={step + 1}>
    <svg className="tour-dimmer" aria-hidden="true"><defs><mask id="topic-spotlight"><rect width="100%" height="100%" fill="white" />{layout.rects.map((rect, index) => <rect key={index} x={rect.left} y={rect.top} width={rect.width} height={rect.height} rx={9} fill="black" />)}</mask></defs><rect width="100%" height="100%" fill="rgba(0,0,0,.56)" mask="url(#topic-spotlight)" /></svg>
    {layout.rects.map((rect, index) => <div key={index} className="tour-ring" style={rect} data-topic-highlight aria-hidden="true" />)}
    <div className="tour-card" ref={card} tabIndex={-1} role="dialog" aria-modal="false" aria-labelledby="topic-title" aria-describedby="topic-description" style={{ left: position.left, top: position.top, width: position.width, maxHeight: layout.height - 24 }}>
      {position.arrow ? <span className="tour-arrow" data-side={position.arrow.side} style={position.arrow.side === "left" || position.arrow.side === "right" ? { top: position.arrow.offset } : { left: position.arrow.offset }} aria-hidden="true" /> : null}
      <div className="tour-copy"><div className="tour-meta"><span>{guide.group}</span><span>{step + 1} / {guide.steps.length}</span></div><div className="tour-progress" aria-hidden="true">{guide.steps.map((_, index) => <span key={index} data-current={index <= step} />)}</div><h2 id="topic-title">{guide.title}</h2><p id="topic-description">{guide.steps[step]}</p>
        {targetAvailable && !last ? <p className="tour-hint">Use the highlighted control, then choose Next.</p> : null}
        {!targetAvailable ? <p className="tour-status" role="status">{["delete", "duplicate", "share"].includes(guide.id) ? "Create or select an environment to see this control." : step > 0 ? "Open the control from the previous step to see it here. You can go Back and try it." : "This control is not available in the current view. Return to the main window to continue."}</p> : null}
        {last && guide.note ? <p className="tour-hint">{guide.note}</p> : null}
      </div>
      <div className="tour-actions"><Button type="button" variant="ghost" onClick={onExit}>Exit</Button><div className="flex items-center gap-2">{step > 0 ? <Button type="button" variant="ghost" onClick={() => setStep(step - 1)}>Back</Button> : null}<Button type="button" className="tour-next" onClick={() => last ? onComplete(guide.id) : setStep(step + 1)}>{last ? "Finish walkthrough" : "Next"}</Button></div></div>
    </div>
  </div>, document.body)
}
