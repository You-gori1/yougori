export interface Size { width: number; height: number }

/** Shrink the letterboxed axis, keeping the complete desktop at one scale.
 * Respect the native guest window's minimum and the current monitor's work area.
 */
export function fittedGuestWindow(inner: Size, viewport: Size, guest: Size, available: Size, minimum: Size = { width: 720, height: 480 }): Size {
  if ([inner, viewport, guest, available, minimum].some(size => !Number.isFinite(size.width) || !Number.isFinite(size.height) || size.width <= 0 || size.height <= 0)) {
    throw new Error("Wait for the guest display to connect before fitting the window.")
  }
  const chrome = { width: Math.max(0, inner.width - viewport.width), height: Math.max(0, inner.height - viewport.height) }
  const least = Math.max((minimum.width - chrome.width) / guest.width, (minimum.height - chrome.height) / guest.height)
  const most = Math.min((available.width - chrome.width) / guest.width, (available.height - chrome.height) / guest.height)
  if (most < least) throw new Error("This desktop's shape cannot fit the current monitor at the window's minimum size. Change the resolution inside the guest.")
  const scale = Math.min(most, Math.max(least, Math.min(viewport.width / guest.width, viewport.height / guest.height)))
  return { width: Math.round(guest.width * scale + chrome.width), height: Math.round(guest.height * scale + chrome.height) }
}

export async function fitWindowToGuestDisplay() {
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("Window fitting is available in the Yougori desktop app.")
  if (document.fullscreenElement) throw new Error("Exit full screen before fitting the window to the desktop.")
  const display = document.querySelector<HTMLElement>("[data-guest-display]")
  const canvas = display?.querySelector("canvas")
  if (!canvas || !display) throw new Error("Wait for the guest display to connect before fitting the window.")
  const { getCurrentWindow, LogicalSize, currentMonitor } = await import("@tauri-apps/api/window")
  const current = getCurrentWindow()
  if (await current.isMaximized()) throw new Error("Restore the window from maximized first, then choose Fit window to desktop.")
  const monitor = await currentMonitor()
  const scaleFactor = monitor?.scaleFactor ?? window.devicePixelRatio
  // Reserve the native title bar/borders, which are outside the webview.
  const [inner, outer] = await Promise.all([current.innerSize(), current.outerSize()])
  const area = monitor?.workArea.size
  const available = {
    width: area ? (area.width - Math.max(0, outer.width - inner.width)) / scaleFactor : window.screen.availWidth,
    height: area ? (area.height - Math.max(0, outer.height - inner.height)) / scaleFactor : window.screen.availHeight,
  }
  if (!display.isConnected || !canvas.isConnected) throw new Error("The guest display changed. Try fitting the window again.")
  const bounds = display.getBoundingClientRect()
  const size = fittedGuestWindow({ width: window.innerWidth, height: window.innerHeight }, bounds, canvas, available)
  await current.setSize(new LogicalSize(size.width, size.height))
}
