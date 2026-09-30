/** The CLI opens in the main view; the toolbar button and the dashboard both
 *  need that one piece of state, without threading it through the shell. */
const listeners = new Set<(open: boolean) => void>()
let open = false

export function cliViewOpen() {
  return open
}

export function setCliView(next: boolean) {
  if (open === next) return
  open = next
  for (const listener of listeners) listener(open)
}

export function toggleCliView() {
  setCliView(!open)
}

export function subscribeCliView(listener: (open: boolean) => void) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}
