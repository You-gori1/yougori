import type { ReactNode } from "react"
import { CircleHelpIcon } from "lucide-react"
import { Tooltip, TooltipPopup, TooltipTrigger } from "@/components/ui/tooltip"

export function ConfigurationHelp({ children, label = "Help" }: { children: ReactNode; label?: string }) {
  return <Tooltip>
    <TooltipTrigger aria-label={label} className="inline-flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring" type="button"><CircleHelpIcon className="size-3.5" aria-hidden="true" /></TooltipTrigger>
    <TooltipPopup role="tooltip" className="max-w-80 text-xs leading-relaxed">{children}</TooltipPopup>
  </Tooltip>
}
