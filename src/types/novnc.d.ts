declare module "@novnc/novnc" {
  interface RfbOptions {
    shared?: boolean
    credentials?: { username?: string; password?: string; target?: string }
    wsProtocols?: string[]
  }

  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, url: string | import("@/lib/remote-display-channel").RemoteDisplayChannel, options?: RfbOptions)
    scaleViewport: boolean
    resizeSession: boolean
    showDotCursor: boolean
    viewOnly: boolean
    focusOnClick: boolean
    background: string
    disconnect(): void
    focus(options?: FocusOptions): void
    sendKey(keysym: number, code: string, down?: boolean): void
    sendCredentials(credentials: { username?: string; password?: string; target?: string }): void
    sendCtrlAltDel(): void
    clipboardPasteFrom(text: string): void
  }
}
