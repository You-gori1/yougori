import { isTauri } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useState } from "react"

export function WindowControls() {
  const [error, setError] = useState("")
  if (!isTauri() || getCurrentWindow().label !== "main") return null
  const run = (action: "minimize" | "toggleMaximize" | "close") => {
    setError("")
    void getCurrentWindow()[action]().catch(() => setError("Window control failed. Please try again."))
  }
  return <div className="relative flex shrink-0 items-center gap-1 border-l pl-2" role="group" aria-label="Window controls">
    <button type="button" title="Minimize" aria-label="Minimize window" className="h-9 w-9 rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring" onClick={() => run("minimize")}>−</button>
    <button type="button" title="Maximize / Restore" aria-label="Maximize or restore window" className="h-9 w-9 rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring" onClick={() => run("toggleMaximize")}>□</button>
    <button type="button" title="Close" aria-label="Close window" className="h-9 w-9 rounded-md text-xl hover:bg-red-800 hover:text-white focus-visible:outline-2 focus-visible:outline-ring" onClick={() => run("close")}>×</button>
    {error ? <span role="alert" className="absolute right-0 top-full z-50 w-56 rounded border bg-background p-2 text-xs">{error}</span> : null}
  </div>
}
