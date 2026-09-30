'use strict';
// Yougori launcher proxy. Published links (localhost, LAN, quick links, saved domains) reach this
// port, and every request is forwarded to the dev server as if it came from localhost, so dev-server
// host checks (Vite server.allowedHosts, webpack-dev-server, Next.js dev origins, SvelteKit CSRF)
// accept them without changes to the project. It also finds the app when it listens on another
// port, follows HTTPS dev servers, and shows a waiting page while the server starts.
// Usage: node proxy.cjs LISTEN_PORT APP_PORT NONCE
const http = require('http');
const https = require('https');
const net = require('net');
const tls = require('tls');
const fs = require('fs');

const listenPort = Number(process.argv[2]);
const appPort = Number(process.argv[3]);
const nonce = process.argv[4] || '';
const mark = (event) => process.stdout.write(`\n[yougori-${nonce}:${event}]\n`);
const started = Date.now();

// The dev server, once found. `secure` is true for HTTPS dev servers (self-signed is fine).
let target = null;
let searching = null;

const HOP = new Set(['connection', 'keep-alive', 'proxy-connection', 'te', 'trailer', 'upgrade', 'x-forwarded-host']);
// Launch serves mutable development URLs, including CSS modules whose URL stays
// the same across restarts. Neither browsers nor publishing CDNs may retain them.
const CACHE_HEADERS = new Set(['cache-control', 'cdn-cache-control', 'cloudflare-cdn-cache-control', 'surrogate-control', 'expires', 'pragma', 'etag', 'last-modified', 'age']);
const NO_STORE = {
  'cache-control': 'no-store, no-cache, max-age=0, must-revalidate',
  'cdn-cache-control': 'no-store',
  'cloudflare-cdn-cache-control': 'no-store',
  'surrogate-control': 'no-store',
  'pragma': 'no-cache',
  'expires': '0',
};

function reachable(host, port) {
  return new Promise((resolve) => {
    const socket = net.connect({ host, port });
    const done = (ok) => { socket.destroy(); resolve(ok); };
    socket.setTimeout(400, () => done(false));
    socket.once('connect', () => done(true));
    socket.once('error', () => done(false));
  });
}

function speaksTls(host, port) {
  return new Promise((resolve) => {
    const socket = tls.connect({ host, port, servername: 'localhost', rejectUnauthorized: false });
    const done = (ok) => { socket.destroy(); resolve(ok); };
    socket.setTimeout(800, () => done(false));
    socket.once('secureConnect', () => done(true));
    socket.once('error', () => done(false));
  });
}

// Listening TCP ports inside the container, from the kernel's socket tables.
function listeningPorts() {
  const ports = new Set();
  for (const table of ['/proc/net/tcp', '/proc/net/tcp6']) {
    let text = '';
    try { text = fs.readFileSync(table, 'utf8'); } catch { continue; }
    for (const line of text.split('\n').slice(1)) {
      const fields = line.trim().split(/\s+/);
      if (fields[3] === '0A') ports.add(parseInt(fields[1].split(':').pop(), 16));
    }
  }
  return ports;
}

async function locate(port) {
  for (const host of ['127.0.0.1', '::1']) {
    if (await reachable(host, port)) return { host, port, secure: await speaksTls(host, port) };
  }
  return null;
}

// Finds the dev server: the configured port first; after a few seconds, any single other port the
// project opened (dev servers that ignore PORT), preferring non-ephemeral ports.
function find() {
  if (!searching) {
    searching = (async () => {
      for (;;) {
        let found = await locate(appPort);
        if (!found && Date.now() - started > 4000) {
          const others = [...listeningPorts()].filter((p) => p !== listenPort && p !== appPort && p !== 24678).sort((a, b) => a - b);
          const preferred = others.filter((p) => p < 32768);
          for (const port of preferred.length ? preferred : others) {
            found = await locate(port);
            if (found) break;
          }
        }
        if (found) {
          const changed = !target || target.port !== found.port || target.secure !== found.secure;
          target = found;
          if (changed) mark(`ready:${found.port}${found.secure ? ':https' : ''}`);
          return target;
        }
        await new Promise((r) => setTimeout(r, 250));
      }
    })().finally(() => { searching = null; });
  }
  return searching;
}

// Request headers as the dev server expects them from a browser on localhost.
// `current` is the target this request started with; `target` may change meanwhile.
function localize(raw, upgrade, current) {
  const headers = {};
  let publicHost = '';
  for (let i = 0; i < raw.length; i += 2) if (raw[i].toLowerCase() === 'host') publicHost = raw[i + 1];
  const local = `localhost:${current.port}`;
  const origin = `${current.secure ? 'https' : 'http'}://${local}`;
  for (let i = 0; i < raw.length; i += 2) {
    const name = raw[i].toLowerCase();
    let value = raw[i + 1];
    if (name === 'host' || (HOP.has(name) && !(upgrade && (name === 'connection' || name === 'upgrade')))) continue;
    if (name === 'origin' || name === 'referer') {
      try {
        const url = new URL(value);
        if (url.host === publicHost) value = name === 'origin' ? origin : origin + url.pathname + url.search + url.hash;
      } catch { /* keep as sent */ }
    }
    headers[name] = name in headers ? `${headers[name]}${name === 'cookie' ? '; ' : ', '}${value}` : value;
  }
  headers.host = local;
  return { headers, publicHost };
}

// Redirects to localhost point back at the address the visitor used.
function publicLocation(raw, publicHost, proto, current) {
  const out = [];
  for (let i = 0; i < raw.length; i += 2) {
    const name = raw[i].toLowerCase();
    if (name === 'connection' || name === 'keep-alive' || CACHE_HEADERS.has(name)) continue;
    let value = raw[i + 1];
    if (name === 'location' && publicHost) {
      try {
        const url = new URL(value);
        if (/^(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)$/.test(url.hostname) && Number(url.port || (url.protocol === 'https:' ? 443 : 80)) === current.port) {
          value = `${proto}://${publicHost}${url.pathname}${url.search}${url.hash}`;
        }
      } catch { /* relative or invalid: unchanged */ }
    }
    out.push(raw[i], value);
  }
  return out.concat(Object.entries(NO_STORE).flat());
}

function waiting(req, res, detail) {
  if (res.headersSent) return res.destroy();
  const html = (req.headers.accept || '').includes('text/html');
  res.writeHead(502, { 'content-type': html ? 'text/html; charset=utf-8' : 'text/plain; charset=utf-8', ...NO_STORE, 'retry-after': '1' });
  res.end(html
    ? `<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="1"><title>Starting…</title><body style="font:15px system-ui;margin:3rem;color:#444"><b>Your dev server is starting.</b><p>This page reloads automatically. Watch the terminal running <code>yougori</code> for errors.</p><p style="color:#888">${detail}</p>`
    : `Your dev server is not answering yet (${detail}). Retry in a moment.\n`);
}

const agent = new http.Agent({ keepAlive: true, maxSockets: 512 });
const secureAgent = new https.Agent({ keepAlive: true, maxSockets: 512, rejectUnauthorized: false });

const server = http.createServer(async (req, res) => {
  if (!target) await Promise.race([find(), new Promise((r) => setTimeout(r, 1500))]);
  const current = target;
  if (!current) return waiting(req, res, `nothing is listening on port ${appPort} yet`);
  const { headers, publicHost } = localize(req.rawHeaders, false, current);
  // Previously cached URLs must get a full response with the new policy, not a
  // 304 that keeps the old body/TTL. Preserve write preconditions and HMR upgrades.
  if (req.method === 'GET' || req.method === 'HEAD') {
    delete headers['if-none-match'];
    delete headers['if-modified-since'];
  }
  const proto = String(req.headers['x-forwarded-proto'] || 'http').split(',')[0].trim() === 'https' ? 'https' : 'http';
  const upstream = (current.secure ? https : http).request({
    host: current.host, port: current.port, method: req.method, path: req.url, headers,
    agent: current.secure ? secureAgent : agent, servername: 'localhost',
  }, (reply) => {
    res.writeHead(reply.statusCode, reply.statusMessage, publicLocation(reply.rawHeaders, publicHost, proto, current));
    reply.pipe(res);
    // A visitor who leaves mid-download must not keep the dev server's connection busy.
    res.on('close', () => { if (!reply.complete) reply.destroy(); });
  });
  upstream.on('error', (error) => {
    // Search again only if no other request already found a new server.
    if (target === current && (error.code === 'ECONNREFUSED' || error.code === 'ECONNRESET')) { target = null; find(); }
    waiting(req, res, error.code || error.message);
  });
  req.pipe(upstream);
});

server.on('upgrade', async (req, socket, head) => {
  socket.on('error', () => socket.destroy());
  if (!target) await Promise.race([find(), new Promise((r) => setTimeout(r, 1500))]);
  const current = target;
  if (!current) return socket.destroy();
  const { headers } = localize(req.rawHeaders, true, current);
  const connect = current.secure
    ? tls.connect({ host: current.host, port: current.port, servername: 'localhost', rejectUnauthorized: false })
    : net.connect({ host: current.host, port: current.port });
  connect.once(current.secure ? 'secureConnect' : 'connect', () => {
    let request = `${req.method} ${req.url} HTTP/1.1\r\n`;
    for (const [name, value] of Object.entries(headers)) request += `${name}: ${value}\r\n`;
    connect.write(request + '\r\n');
    if (head && head.length) connect.write(head);
    socket.pipe(connect).pipe(socket);
  });
  connect.on('error', () => socket.destroy());
  socket.on('close', () => connect.destroy());
});

server.requestTimeout = 0;
server.headersTimeout = 60000;
server.keepAliveTimeout = 65000;
server.on('error', (error) => {
  process.stdout.write(`\n[yougori] Cannot publish port ${listenPort}: ${error.message}\n`);
  process.exit(1);
});
server.listen(listenPort, '0.0.0.0', () => find());
// The supervisor restarted the dev server: report ready again once it listens.
process.on('SIGUSR2', () => { target = null; find(); });
