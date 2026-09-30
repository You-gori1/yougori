import { remoteAccessApi } from "@/api/remote-access-api"

// noVNC supports a WebSocket-like channel. Native IPC supplies authenticated
// HTTP transport so credentials never enter a WebSocket URL or browser storage.
export class RemoteDisplayChannel {
  binaryType = "arraybuffer"
  protocol = "binary"
  readyState = 0
  onopen: (() => void) | null = null
  onmessage: ((event: { data: ArrayBuffer }) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null
  private queued = 0
  private writing = Promise.resolve()
  private offset = 0
  private timer: ReturnType<typeof setTimeout> | undefined
  constructor(private environmentId: string, private displayId: string, private report: (error: unknown) => void) {}
  open() { if (this.readyState !== 0) return; this.readyState = 1; this.onopen?.(); void this.read() }
  send(value: ArrayBuffer | ArrayBufferView) {
    if (this.readyState !== 1) return
    const bytes = value instanceof ArrayBuffer ? new Uint8Array(value.slice(0)) : new Uint8Array(value.buffer, value.byteOffset, value.byteLength).slice()
    this.queued += bytes.length
    if (this.queued > 256 * 1024) { this.fail(new Error("Desktop input is congested. Reconnect to resume.")); return }
    this.writing = this.writing.then(async () => {
      for (let offset = 0; offset < bytes.length && this.readyState === 1; offset += 65536) {
        const data = btoa(Array.from(bytes.subarray(offset, offset + 65536), b => String.fromCharCode(b)).join(""))
        await remoteAccessApi.desktop(this.environmentId, { action: "write", displayId: this.displayId, data })
      }
    }).catch(e => this.fail(e)).finally(() => { this.queued -= bytes.length })
  }
  private async read() {
    try {
      const result = await remoteAccessApi.desktop<{ data: string; offset: number; done: boolean }>(this.environmentId, { action: "read", displayId: this.displayId, offset: this.offset })
      if (this.readyState !== 1) return
      this.offset = result.offset
      if (result.data) { const bytes = Uint8Array.from(atob(result.data), c => c.charCodeAt(0)); this.onmessage?.({ data: bytes.buffer }) }
      if (result.done) { this.fail(new Error("Remote desktop closed")); return }
      this.timer = setTimeout(() => void this.read(), result.data ? 0 : 30)
    } catch (e) { this.fail(e) }
  }
  private fail(error: unknown) { if (this.readyState === 3) return; this.report(error); this.onerror?.(new Event("error")); this.close() }
  close() {
    if (this.readyState === 3) return
    this.readyState = 3; clearTimeout(this.timer)
    void remoteAccessApi.desktop(this.environmentId, { action: "close", displayId: this.displayId }).catch(() => undefined)
    this.onclose?.({ code: 1000, reason: "Session closed", wasClean: true })
  }
}
