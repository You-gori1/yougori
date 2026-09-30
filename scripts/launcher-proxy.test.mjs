// Tests the launcher's in-container proxy against a dev server with host checks.
import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import http from "node:http"
import net from "node:net"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { mkdtemp, realpath, writeFile, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createServer } from "vite"

const proxyScript = fileURLToPath(new URL("../cli/src/launcher/assets/proxy.cjs", import.meta.url))

function freePort() {
  return new Promise(resolve => { const s = net.createServer().listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => resolve(port)) }) })
}

// A dev server that, like Vite, only answers requests addressed to localhost.
function devServer(port) {
  const seen = []
  const server = http.createServer((req, res) => {
    seen.push({ host: req.headers.host, origin: req.headers.origin, forwardedHost: req.headers["x-forwarded-host"] })
    if (!req.headers.host.startsWith("localhost:")) { res.writeHead(403); return res.end(`Blocked request. This host ("${req.headers.host}") is not allowed.`) }
    if (req.url === "/redirect") { res.writeHead(302, { location: `http://localhost:${port}/next?x=1` }); return res.end() }
    res.writeHead(200, { "content-type": "text/plain", "set-cookie": ["a=1", "b=2"] })
    res.end(`ok ${req.method} ${req.url}`)
  })
  server.on("upgrade", (req, socket) => {
    seen.push({ upgrade: true, host: req.headers.host, origin: req.headers.origin })
    // Closing the client resets this socket on Windows; that is expected, not a failure.
    socket.on("error", () => {})
    if (!req.headers.host.startsWith("localhost:")) return socket.destroy()
    socket.write("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
    socket.on("data", d => socket.write(`echo:${d}`))
  })
  return new Promise(resolve => server.listen(port, "127.0.0.1", () => resolve({ server, seen })))
}

function request(port, path, headers, method = 'GET') {
  return new Promise((resolve, reject) => {
    http.request({ host: "127.0.0.1", port, path, headers, method }, res => {
      let body = ""; res.on("data", d => body += d); res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body }))
    }).on("error", reject).end()
  })
}

async function startProxy(listen, app) {
  const child = spawn(process.execPath, [proxyScript, String(listen), String(app), "abc123"], { stdio: ["ignore", "pipe", "inherit"] })
  let output = ""
  child.stdout.on("data", d => output += d)
  for (let i = 0; i < 100 && !output.includes("[yougori-abc123:ready"); i++) await new Promise(r => setTimeout(r, 50))
  return { child, output: () => output }
}

test("public hosts reach a dev server that only allows localhost", async t => {
  const [app, listen] = [await freePort(), await freePort()]
  const { server, seen } = await devServer(app)
  const proxy = await startProxy(listen, app)
  t.after(() => { proxy.child.kill(); server.close() })
  assert.match(proxy.output(), new RegExp(`\\[yougori-abc123:ready:${app}\\]`))

  const host = "camping-authorization-willing-activities.trycloudflare.com"
  const reply = await request(listen, "/page?q=1", { host, origin: `https://${host}`, "x-forwarded-proto": "https", "x-forwarded-host": "evil.example" })
  assert.equal(reply.status, 200)
  assert.equal(reply.body, "ok GET /page?q=1")
  assert.deepEqual(reply.headers["set-cookie"], ["a=1", "b=2"])
  assert.deepEqual(seen.at(-1), { host: `localhost:${app}`, origin: `http://localhost:${app}`, forwardedHost: undefined })

  const other = await request(listen, "/", { host, origin: "https://elsewhere.example" })
  assert.equal(other.status, 200)
  assert.equal(seen.at(-1).origin, "https://elsewhere.example", "a genuine cross-origin request keeps its origin")

  const redirect = await request(listen, "/redirect", { host, "x-forwarded-proto": "https" })
  assert.equal(redirect.headers.location, `https://${host}/next?x=1`)

  const echoed = await new Promise((resolve, reject) => {
    const socket = net.connect(listen, "127.0.0.1", () => socket.write(`GET /hmr HTTP/1.1\r\nHost: ${host}\r\nOrigin: https://${host}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`))
    let data = ""
    socket.on("data", d => { data += d; if (data.includes("\r\n\r\n") && !data.includes("echo:")) socket.write("ping"); if (data.includes("echo:ping")) { socket.destroy(); resolve(data) } })
    socket.on("error", reject)
  })
  assert.match(echoed, /101 Switching Protocols/)
  assert.deepEqual(seen.at(-1), { upgrade: true, host: `localhost:${app}`, origin: `http://localhost:${app}` })
})

test("a dev server restart during an in-flight redirect does not crash the proxy", async t => {
  const [app, listen] = [await freePort(), await freePort()]
  let releaseRedirect
  const redirectReady = new Promise(resolve => { releaseRedirect = resolve })
  const server = http.createServer((req, res) => {
    if (req.url === "/slow-redirect") return void redirectReady.then(() => { res.writeHead(302, { location: `http://localhost:${app}/after` }); res.end() })
    if (req.url === "/restart") { server.close(); req.socket.destroy(); setTimeout(releaseRedirect, 300); return }
    res.end("ok")
  })
  await new Promise(resolve => server.listen(app, "127.0.0.1", resolve))
  const proxy = await startProxy(listen, app)
  t.after(() => { proxy.child.kill(); server.closeAllConnections() })
  const host = "preview.example"
  const redirect = request(listen, "/slow-redirect", { host, "x-forwarded-proto": "https" })
  await new Promise(r => setTimeout(r, 100))
  await request(listen, "/restart", { host }).catch(() => undefined)
  assert.equal((await redirect).headers.location, `https://${host}/after`)
  await new Promise(r => setTimeout(r, 100))
  assert.equal(proxy.child.exitCode, null, "the proxy must keep serving other visitors")
})

test("a waiting page is shown until the dev server starts", async t => {
  const [app, listen] = [await freePort(), await freePort()]
  const proxy = spawn(process.execPath, [proxyScript, String(listen), String(app), "n"], { stdio: "ignore" })
  t.after(() => proxy.kill())
  await new Promise(r => setTimeout(r, 400))
  const reply = await request(listen, "/", { accept: "text/html" })
  assert.equal(reply.status, 502)
  assert.match(reply.headers['cache-control'], /no-store/)
  assert.equal(reply.headers['cloudflare-cdn-cache-control'], 'no-store')
  assert.match(reply.body, /dev server is starting/)
  const { server } = await devServer(app)
  t.after(() => server.close())
  let ok = null
  for (let i = 0; i < 40 && ok?.status !== 200; i++) { await new Promise(r => setTimeout(r, 100)); ok = await request(listen, "/x", {}) }
  assert.equal(ok.status, 200)
})

test("development assets stay fresh across restarts and stale CDN revalidation", { timeout: 20000 }, async t => {
  const app = await freePort(), listen = await freePort()
  let server
  const seen = []
  const start = body => new Promise(resolve => {
    server = http.createServer((req, res) => {
      seen.push({ method: req.method, headers: req.headers })
      // Model a dev server and intermediary that would otherwise reuse a stale
      // body when a browser/CDN sends its cached validators after a restart.
      res.setHeader('Cache-Control', 'public, max-age=14400, immutable')
      res.setHeader('CDN-Cache-Control', 'max-age=14400')
      res.setHeader('Cloudflare-CDN-Cache-Control', 'max-age=14400')
      res.setHeader('Surrogate-Control', 'max-age=14400')
      res.setHeader('Expires', 'Wed, 01 Jan 2031 00:00:00 GMT')
      res.setHeader('ETag', '"unchanged-validator"')
      res.setHeader('Last-Modified', 'Mon, 28 Sep 2026 00:00:00 GMT')
      res.setHeader('Age', '600')
      if (req.headers['if-none-match'] || req.headers['if-modified-since']) {
        res.writeHead(304); return res.end()
      }
      res.end(body)
    }).listen(app, '127.0.0.1', resolve)
  })
  const close = () => new Promise(resolve => { server.close(resolve); server.closeAllConnections() })
  await start('before')
  t.after(() => close())
  const proxy = await startProxy(listen, app)
  t.after(() => proxy.child.kill())
  const assertUncached = reply => {
    assert.equal(reply.status, 200)
    assert.match(reply.headers['cache-control'], /no-store/)
    assert.doesNotMatch(reply.headers['cache-control'], /public|immutable|14400/)
    for (const name of ['cdn-cache-control', 'cloudflare-cdn-cache-control', 'surrogate-control']) assert.equal(reply.headers[name], 'no-store')
    for (const name of ['etag', 'last-modified', 'age']) assert.equal(reply.headers[name], undefined)
    assert.equal(reply.headers.expires, '0')
  }
  for (const host of ['localhost', '192.168.1.10', 'preview.trycloudflare.com', 'app.example.com']) {
    for (const path of ['/', '/src/style.css', '/src/main.js', '/logo.png']) {
      const reply = await request(listen, path, { host })
      assertUncached(reply)
      assert.equal(reply.body, 'before')
    }
  }
  await close()
  await start('after restart')
  for (const method of ['GET', 'HEAD']) {
    const reply = await request(listen, '/src/style.css', {
      host: 'app.example.com',
      'if-none-match': '"unchanged-validator"',
      'if-modified-since': 'Mon, 28 Sep 2026 00:00:00 GMT',
    }, method)
    assertUncached(reply)
    assert.equal(reply.body, method === 'HEAD' ? '' : 'after restart')
    assert.equal(seen.at(-1).headers['if-none-match'], undefined)
    assert.equal(seen.at(-1).headers['if-modified-since'], undefined)
  }
  // API concurrency checks must not be disabled by a development-cache fix.
  await request(listen, '/api/item', {
    'if-match': '"revision-1"', 'if-none-match': '*',
    'if-unmodified-since': 'Mon, 28 Sep 2026 00:00:00 GMT',
  }, 'PUT')
  assert.equal(seen.at(-1).headers['if-match'], '"revision-1"')
  assert.equal(seen.at(-1).headers['if-none-match'], '*')
  assert.equal(seen.at(-1).headers['if-unmodified-since'], 'Mon, 28 Sep 2026 00:00:00 GMT')
})

test("real Vite serves public hosts and delivers hot reload through the proxy", { timeout: 20000 }, async t => {
  const root = await realpath(await mkdtemp(join(tmpdir(), "yougori-vite-")))
  const app = await freePort(), listen = await freePort()
  await writeFile(join(root, "index.html"), "<h1>before</h1>")
  // Poll this disposable fixture on Windows: hosted runners can expose TEMP
  // through path aliases that trigger libuv's native fs-event assertion.
  const vite = await createServer({ root, configFile: false, logLevel: "silent", server: { host: "127.0.0.1", port: app, strictPort: true, fs: { allow: [root] }, watch: { usePolling: process.platform === "win32", interval: 50 } } })
  await vite.listen()
  t.after(async () => { await vite.close(); await rm(root, { recursive: true, force: true }) })
  const proxy = await startProxy(listen, app)
  t.after(() => proxy.child.kill())
  const direct = await request(app, "/", { host: "public.example" })
  assert.equal(direct.status, 403)
  const published = await request(listen, "/", { host: "public.example" })
  assert.equal(published.status, 200, published.body)
  assert.match(published.body, /before/)
  const socket = new WebSocket(`ws://127.0.0.1:${listen}/?token=${vite.config.webSocketToken}`, "vite-hmr")
  t.after(() => socket.close())
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Vite did not deliver a hot-reload event")), 10000)
    socket.addEventListener("error", () => { clearTimeout(timer); reject(new Error("Vite websocket failed")) })
    socket.addEventListener("message", async ({ data }) => {
      const message = JSON.parse(data)
      if (message.type === "connected") await writeFile(join(root, "index.html"), "<h1>after</h1>")
      if (message.type === "full-reload") { clearTimeout(timer); resolve() }
    })
  })
  assert.match((await request(listen, "/", { host: "public.example" })).body, /after/)
})
