import { Component, type ReactNode } from "react"

/** Keep a failed render or lazy-loaded view from leaving a blank desktop window. */
export class AppErrorBoundary extends Component<{ children: ReactNode; onReload?: () => void }, { failed: boolean }> {
  state = { failed: false }

  static getDerivedStateFromError() { return { failed: true } }

  render() {
    if (!this.state.failed) return this.props.children
    return <main className="grid min-h-screen place-items-center bg-background p-8 text-foreground">
      <section className="flex w-full max-w-lg flex-col gap-4" role="alert">
        <h1 className="text-lg font-semibold">The Yougori interface needs to reload</h1>
        <p className="text-sm leading-relaxed text-muted-foreground">An unexpected display error prevented this window from opening correctly.</p>
        <p className="text-sm leading-relaxed text-muted-foreground">Reloading refreshes the interface. It does not delete or factory-reset environments. Commands in this window’s terminal sessions may be interrupted.</p>
        <button className="self-start rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring" type="button" onClick={() => this.props.onReload ? this.props.onReload() : window.location.reload()}>Reload window</button>
        <p className="text-xs leading-relaxed text-muted-foreground">If it happens again, report which action triggered it and your Yougori version. Avoid deleting environments to fix an interface error.</p>
      </section>
    </main>
  }
}
