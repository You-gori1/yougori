import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { App } from "@/app"
import { AppErrorBoundary } from "@/components/app-error-boundary"
import "@/styles.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppErrorBoundary><App /></AppErrorBoundary>
  </StrictMode>,
)
