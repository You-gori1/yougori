import { useEffect, useState } from "react"
import { CheckIcon, ChevronDownIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Sheet, SheetDescription, SheetHeader, SheetPanel, SheetPopup, SheetTitle } from "@/components/ui/sheet"
import { instructionGuides, instructionGuideIds } from "@/lib/instruction-guides"
import { startInstructions } from "@/lib/instructions-tour"
import { setTopicWalkthrough, useTopicWalkthrough } from "@/lib/topic-walkthrough"
import { TopicWalkthrough } from "@/components/topic-walkthrough"

const progressKey = "yougori.instructions.walkthroughs.v1"
const groups = ["Start here", "Create environments", "Connect and publish", "Use and manage"] as const

function readProgress(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(progressKey) || "[]")
    return Array.isArray(value) ? [...new Set(value.filter((id): id is string => typeof id === "string" && instructionGuideIds.has(id)))] : []
  } catch { return [] }
}

export function InstructionGuides() {
  const [open, setOpen] = useState(false)
  const [completed, setCompleted] = useState<string[]>(readProgress)
  const [selected, setSelected] = useState<string | null>(null)
  const active = useTopicWalkthrough()
  const activeGuide = instructionGuides.find(guide => guide.id === active)
  const remaining = instructionGuides.length - completed.length

  useEffect(() => {
    const show = () => setOpen(true)
    window.addEventListener("yougori-open-instruction-guides", show)
    return () => window.removeEventListener("yougori-open-instruction-guides", show)
  }, [])

  function finishGuide(id: string) {
    const next = completed.includes(id) ? completed : [...completed, id]
    setCompleted(next)
    try { localStorage.setItem(progressKey, JSON.stringify(next)) } catch { /* Guides remain usable without storage. */ }
    setTopicWalkthrough(null)
  }

  return <>
    <Button data-tour="instructions" size="xs" variant="ghost" className="workspace-footer-reclaim" onClick={() => setOpen(true)} type="button">
      Instructions{remaining > 0 ? <span aria-hidden="true" className="ml-1 rounded-full bg-primary/12 px-1.5 text-[10px] text-primary">{remaining}</span> : null}
    </Button>
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetPopup side="right" className="sm:max-w-[36rem]" aria-label="Yougori instructions">
        <SheetHeader className="border-b pb-4 pr-14">
          <SheetTitle>Instructions</SheetTitle>
          <SheetDescription>Follow these required walkthroughs in the app. Your progress is saved.</SheetDescription>
          <p className="pt-1 text-xs text-muted-foreground" role="status">{remaining === 0 ? "All walkthroughs complete" : `${completed.length} of ${instructionGuides.length} walkthroughs complete · ${remaining} remaining`}</p>
          <div className="h-1.5 overflow-hidden rounded-full bg-muted" aria-hidden="true"><div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${completed.length / instructionGuides.length * 100}%` }} /></div>
        </SheetHeader>
        <SheetPanel className="space-y-5 px-4 py-4 sm:px-6" scrollFade={false}>
          <div className="rounded-xl border bg-muted/40 p-3 text-sm">
            <p className="font-medium">First-time walkthrough</p>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">Replay the existing guided tour to create a container, try the terminal, and publish a Hello World website.</p>
            <Button size="sm" variant="outline" className="mt-3" onClick={() => { setOpen(false); startInstructions() }}>Replay walkthrough</Button>
          </div>
          {groups.map(group => <section key={group} aria-label={group} className="space-y-2">
            <h3 className="px-1 text-xs font-semibold uppercase tracking-[.08em] text-muted-foreground">{group}</h3>
            <div className="overflow-hidden rounded-xl border bg-card">
              {instructionGuides.filter(guide => guide.group === group).map((guide, index) => {
                const expanded = selected === guide.id
                const finished = completed.includes(guide.id)
                return <div key={guide.id} className={index ? "border-t" : undefined}>
                  <button type="button" className="flex w-full items-center gap-3 px-3 py-3 text-left text-sm hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring" aria-label={guide.title} aria-expanded={expanded} aria-controls={`instruction-guide-${guide.id}`} onClick={() => setSelected(expanded ? null : guide.id)}>
                    <span className={`grid size-5 shrink-0 place-items-center rounded-full border text-[10px] ${finished ? "border-primary bg-primary text-primary-foreground" : "border-muted-foreground/40 text-muted-foreground"}`}>{finished ? <CheckIcon aria-hidden="true" className="size-3" /> : instructionGuides.indexOf(guide) + 1}</span>
                    <span className="min-w-0 flex-1 font-medium">{guide.title}</span><ChevronDownIcon aria-hidden="true" className={`size-4 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-180" : ""}`} />
                  </button>
                  {expanded ? <div id={`instruction-guide-${guide.id}`} className="border-t bg-muted/20 px-4 pb-4 pt-3">
                    <p className="text-sm leading-6 text-muted-foreground">Follow {guide.steps.length} highlighted steps on the actual Yougori controls. You decide whether to create, connect or delete anything.</p>
                    {guide.note ? <p className="mt-3 rounded-lg border bg-background px-3 py-2 text-xs leading-5 text-muted-foreground">{guide.note}</p> : null}
                    <Button size="sm" variant={finished ? "outline" : "default"} className="mt-4" onClick={() => { setOpen(false); setTopicWalkthrough(guide.id) }}>{finished ? "Replay walkthrough" : "Start walkthrough"}</Button>
                  </div> : null}
                </div>
              })}
            </div>
          </section>)}
        </SheetPanel>
      </SheetPopup>
    </Sheet>
    {activeGuide ? <TopicWalkthrough key={activeGuide.id} guide={activeGuide} onComplete={finishGuide} onExit={() => setTopicWalkthrough(null)} /> : null}
  </>
}
