import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { getEventListeners } from "node:events"
import { createServer } from "node:http"
import test from "node:test"
import { waitForVnc } from "./wait-for-vnc.mjs"

// Loopback-only display fixtures. No QEMU, guest disks or desktop app involved.
async function display(t, connected) {
  const server = createServer(), sockets = new Set()
  server.on("connection", socket => {
    sockets.add(socket)
    socket.on("error", () => {})
    socket.on("close", () => sockets.delete(socket))
  })
  server.on("upgrade", (request, socket) => {
    const key = createHash("sha1").update(request.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64")
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${key}\r\nSec-WebSocket-Protocol: binary\r\n\r\n`)
    connected(socket)
  })
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve))
  t.after(async () => {
    for (const socket of sockets) socket.destroy()
    await new Promise(resolve => server.close(resolve))
  })
  return `ws://127.0.0.1:${server.address().port}`
}

function frame(socket, text) {
  const payload = Buffer.from(text)
  assert.ok(payload.length < 126)
  socket.write(Buffer.concat([Buffer.from([0x82, payload.length]), payload]))
}

test("VNC readiness waits for the full protocol greeting after the socket opens", async t => {
  const connected = Promise.withResolvers()
  const url = await display(t, connected.resolve)
  let ready = false
  const waiting = waitForVnc(url).then(() => { ready = true })
  const socket = await connected.promise
  assert.equal(ready, false)
  frame(socket, "RFB 003.")
  frame(socket, "008\n")
  await waiting
  assert.equal(ready, true)
})

test("VNC readiness tolerates startup disconnects until the display responds", async t => {
  let attempts = 0
  const url = await display(t, socket => {
    if (++attempts === 1) socket.destroy()
    else frame(socket, "RFB 003.008\n")
  })
  await waitForVnc(url)
  assert.equal(attempts, 2)
})

test("a stopped QEMU cancels VNC readiness and releases its abort listener", async t => {
  const connected = Promise.withResolvers(), stopped = new AbortController()
  const url = await display(t, connected.resolve)
  const reason = new Error("Disposable QEMU exited (1)")
  const result = assert.rejects(waitForVnc(url, { signal: stopped.signal }), error => error === reason)
  await connected.promise
  stopped.abort(reason)
  await result
  assert.equal(getEventListeners(stopped.signal, "abort").length, 0)
})

test("an open display socket without a greeting has a bounded readiness timeout", async t => {
  const url = await display(t, () => {})
  await assert.rejects(waitForVnc(url, { timeoutMs: 150 }), /VNC did not become ready within 150ms: No VNC greeting received/)
})

test("another protocol on the port cannot count as a ready VNC display", async t => {
  let attempts = 0
  const url = await display(t, socket => { attempts++; frame(socket, "NOT A VNC!!!") })
  await assert.rejects(waitForVnc(url), /Unexpected VNC greeting/)
  assert.equal(attempts, 1)
})
