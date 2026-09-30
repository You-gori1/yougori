import { cn } from "@/lib/utils"
import { statusLabel } from "@/lib/domain"
import type { EnvironmentStatus } from "@/types/platform"

export function Status({ status, compact = false }: { status: EnvironmentStatus; compact?: boolean }) {
  return (
    <span className={cn("inline-flex items-center gap-2 text-sm", compact && "text-xs", status === "error" ? "text-destructive" : "text-muted-foreground")}>
      <span
        aria-hidden="true"
        className={cn(
          "size-1.5 rounded-full bg-muted-foreground/50",
          status === "running" && "bg-primary",
          status === "provisioning" && "animate-pulse bg-primary",
          status === "error" && "bg-destructive",
        )}
      />
      {statusLabel[status]}
    </span>
  )
}

export function Availability({ ready, label }: { ready: boolean; label?: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-xs text-muted-foreground">
      <span aria-hidden="true" className={cn("size-1.5 rounded-full bg-muted-foreground/40", ready && "bg-primary")} />
      {label ?? (ready ? "Ready" : "Unavailable")}
    </span>
  )
}
