import type RFB from "@novnc/novnc"

interface GuestKey { token: string; code: string; keysym: number; down: boolean }

/** Native Windows-key capture is opt-in to this visible canvas, never the toolbar. */
export function bindGuestKeyboard(target: HTMLElement, client: Pick<RFB, "sendKey" | "focus">, onError: (message: string) => void) {
  if (!("__TAURI_INTERNALS__" in window) || !/Win/i.test(navigator.platform)) return () => undefined
  const token = crypto.randomUUID()
  let disposed = false, inside = false
  let unlisten: (() => void) | undefined
  let invoke: typeof import("@tauri-apps/api/core").invoke | undefined
  let pending = Promise.resolve()
  const releaseKeys = () => {
    client.sendKey(0xffeb, "MetaLeft", false)
    client.sendKey(0xffec, "MetaRight", false)
  }
  const update = () => {
    const enabled = !disposed && inside && document.hasFocus()
    const rect = target.getBoundingClientRect(), ratio = window.devicePixelRatio || 1
    const bounds = enabled ? { left: Math.max(0, rect.left * ratio), top: Math.max(0, rect.top * ratio), right: rect.right * ratio, bottom: rect.bottom * ratio } : null
    pending = pending.then(async () => {
      if (invoke) await invoke("set_guest_keyboard_capture", { token, bounds: disposed ? null : bounds })
    }).catch((reason: unknown) => { if (!disposed) onError(`Windows-key capture unavailable: ${reason instanceof Error ? reason.message : String(reason)}`) })
  }
  const enter = () => {
    inside = true
    if (document.hasFocus()) client.focus()
    update()
  }
  const leave = () => { inside = false; releaseKeys(); update() }
  const focus = () => { inside = target.matches(":hover"); if (inside) client.focus(); update() }
  const blur = () => { releaseKeys(); update() }
  const focusOut = (event: FocusEvent) => { if (!target.contains(event.relatedTarget as Node | null)) leave() }
  target.addEventListener("pointerenter", enter)
  target.addEventListener("pointerleave", leave)
  target.addEventListener("focusout", focusOut)
  window.addEventListener("focus", focus)
  window.addEventListener("blur", blur)
  window.addEventListener("resize", update)
  const observer = new ResizeObserver(update)
  observer.observe(target)
  void Promise.all([import("@tauri-apps/api/core"), import("@tauri-apps/api/window")]).then(async ([core, windows]) => {
    if (disposed) return
    invoke = core.invoke
    unlisten = await windows.getCurrentWindow().listen<GuestKey>("guest-system-key", ({ payload }) => {
      if (disposed || payload.token !== token) return
      if (!payload.down || (inside && document.hasFocus())) client.sendKey(payload.keysym, payload.code, payload.down)
    })
    if (disposed) { unlisten(); return }
    inside = target.matches(":hover")
    update()
  }).catch((reason: unknown) => { if (!disposed) onError(`Windows-key capture unavailable: ${String(reason)}`) })
  return () => {
    disposed = true
    leave()
    unlisten?.()
    observer.disconnect()
    target.removeEventListener("pointerenter", enter)
    target.removeEventListener("pointerleave", leave)
    target.removeEventListener("focusout", focusOut)
    window.removeEventListener("focus", focus)
    window.removeEventListener("blur", blur)
    window.removeEventListener("resize", update)
  }
}
