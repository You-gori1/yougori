import type RFB from "@novnc/novnc"

/** Request a real guest resolution change, never stretch or crop its framebuffer.
 * Only the foreground viewer controls resolution while windows still mirror one
 * desktop. noVNC negotiates support with the server and throttles resize requests.
 */
export function bindGuestDisplaySizing(client: Pick<RFB, "scaleViewport" | "resizeSession">, automatic: boolean) {
  client.scaleViewport = true
  const update = () => {
    client.resizeSession = automatic && document.hasFocus() && document.visibilityState === "visible"
  }
  const release = () => { client.resizeSession = false }
  window.addEventListener("focus", update)
  window.addEventListener("blur", release)
  document.addEventListener("visibilitychange", update)
  update()
  return () => {
    window.removeEventListener("focus", update)
    window.removeEventListener("blur", release)
    document.removeEventListener("visibilitychange", update)
    release()
  }
}
