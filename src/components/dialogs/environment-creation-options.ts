import { BoxIcon, CircuitBoardIcon, GitBranchIcon, MonitorIcon, type LucideIcon } from "lucide-react"
import type { EnvironmentKind } from "@/types/platform"

export const isolationPresentation: Record<EnvironmentKind, { icon: LucideIcon; tag: string; description: string; detail: string }> = {
  cloud: { icon: MonitorIcon, tag: "Remote", description: "Connect an existing server.", detail: "Choose Cloud environment in New environment" },
  container: { icon: BoxIcon, tag: "Lean & focused", description: "Services, development tools, and Linux workspaces.", detail: "OCI image · shared guest kernel" },
  microVm: { icon: CircuitBoardIcon, tag: "Dedicated kernel", description: "Small Linux guests with a separate kernel.", detail: "Direct boot · minimal virtual hardware" },
  fullVm: { icon: MonitorIcon, tag: "Full desktop", description: "A complete operating system, on your terms.", detail: "Your ISO or disk · QEMU" },
  computerBranch: { icon: GitBranchIcon, tag: "Unavailable", description: "Computer branching is currently unavailable.", detail: "Choose another isolation type" },
}
