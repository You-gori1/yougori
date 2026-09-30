import type { CommandResult } from "@/types/platform"
import { tourWebsitePage } from "./tour-website-page"

export const tourWebsitePort = 3000
function marker(run: string) {
  if (!/^[\w-]{1,80}$/.test(run)) throw new Error("Invalid tutorial ID")
  return `yougori-hello-${run}`
}
const quote = (value: string) => `'${value.replaceAll("'", `'"'"'`)}'`
const pythonCommand = (code: string) => `python3 -c ${quote(`exec(${JSON.stringify(code)})`)}`

// Serve only our fixed page. Request paths can never select a file or reflect
// query strings, credentials or container/host data into the response.
export function helloWebsitePython(run: string, host = "0.0.0.0", port = tourWebsitePort, savedPage = false) {
  if (!["0.0.0.0", "127.0.0.1"].includes(host) || !Number.isInteger(port) || port < 0 || port > 65535) throw new Error("Invalid demo listener")
  const page = tourWebsitePage(marker(run))
  return `from http.server import BaseHTTPRequestHandler, HTTPServer
from socketserver import TCPServer
from pathlib import Path
page = ${savedPage ? "Path(__file__).with_name('index.html').read_bytes()" : `${JSON.stringify(page)}.encode('utf-8')`}
class Hello(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(page)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Security-Policy', "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.send_header('X-Yougori-Tutorial', '${marker(run)}')
        self.end_headers()
        self.wfile.write(page)
    def log_message(self, *args):
        pass
class HelloServer(HTTPServer):
    def server_bind(self):
        TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]
server = HelloServer(('${host}', ${port}), Hello)
print('Hello World ready on port ' + str(server.server_port) + '.', flush=True)
server.serve_forever()`
}

export function helloWebsiteSetupPython(run: string) {
  const token = marker(run)
  return `import errno, fcntl, os, subprocess, sys, time, urllib.error, urllib.request
from pathlib import Path
token = '${token}'
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
def ready():
    try:
        with opener.open('http://127.0.0.1:${tourWebsitePort}/', timeout=3) as response:
            if response.headers.get('X-Yougori-Tutorial') == token and token.encode() in response.read(32768):
                return True
            raise RuntimeError('Port 3000 is already used by another website. Stop that app or use a fresh tutorial container, then retry.')
    except urllib.error.URLError as error:
        if getattr(error.reason, 'errno', None) == errno.ECONNREFUSED:
            return False
        raise RuntimeError('Could not check port 3000. No existing service was changed.') from error
directory = Path.home() / '.local/share/yougori/tutorials/${run}'
directory.mkdir(mode=0o700, parents=True, exist_ok=True)
with (directory / 'build.lock').open('a') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    if not ready():
        (directory / 'index.html').write_text(${JSON.stringify(tourWebsitePage(token))}, encoding='utf-8')
        script = directory / 'server.py'
        script.write_text(${JSON.stringify(helloWebsitePython(run, "0.0.0.0", tourWebsitePort, true))}, encoding='utf-8')
        with (directory / 'server.log').open('ab') as log:
            child = subprocess.Popen([sys.executable, '-u', str(script)], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True, close_fds=True)
        try:
            for attempt in range(40):
                if child.poll() is not None:
                    raise RuntimeError('The website could not start. Check ' + str(directory / 'server.log'))
                if ready():
                    (directory / 'server.pid').write_text(str(child.pid), encoding='ascii')
                    break
                time.sleep(0.15)
            else:
                raise RuntimeError('The website did not become ready. Retry the website setup.')
        except BaseException:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)
            raise
print(token)`
}

export function helloWebsiteCommand(run: string) {
  const script = `set -e; od_demo_root() { if [ "$(id -u)" = 0 ]; then "$@"; elif command -v sudo >/dev/null 2>&1; then sudo -n "$@"; else printf '%s\\n' 'Python setup needs root or sudo. Use the default tutorial container.' >&2; return 1; fi; }; if ! command -v python3 >/dev/null 2>&1; then if command -v apk >/dev/null 2>&1; then od_demo_root apk add --no-cache python3; elif command -v apt-get >/dev/null 2>&1; then od_demo_root apt-get update && od_demo_root apt-get install -y --no-install-recommends python3; elif command -v dnf >/dev/null 2>&1; then od_demo_root dnf install -y python3; elif command -v microdnf >/dev/null 2>&1; then od_demo_root microdnf install -y python3; elif command -v yum >/dev/null 2>&1; then od_demo_root yum install -y python3; elif command -v zypper >/dev/null 2>&1; then od_demo_root zypper --non-interactive install python3; else printf '%s\\n' 'Install Python 3 in this container, then retry.' >&2; exit 1; fi; fi; exec ${pythonCommand(helloWebsiteSetupPython(run))}`
  return `sh -c ${quote(script)}`
}

export function helloWebsiteCheck(run: string) {
  return pythonCommand(`import urllib.request
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open('http://127.0.0.1:${tourWebsitePort}/', timeout=3) as response:
    page = response.read(32768)
if b'${marker(run)}' not in page:
    raise SystemExit('Port ${tourWebsitePort} is not this tutorial website. Do not publish it.')
print('${marker(run)}')`)
}
export async function verifyHelloWebsite(execute: (id: string, command: string) => Promise<CommandResult>, id: string, run: string) {
  const result = await execute(id, helloWebsiteCheck(run))
  if (result.exitCode !== 0 || result.stdout.trim() !== marker(run)) throw new Error("Hello World is not ready on port 3000. Check that the tutorial container is running. If the port is already in use, do not publish that other app; stop it yourself or use a fresh tutorial container, then retry.")
}

const builds = new Map<string, Promise<void>>()
export function buildHelloWebsite(execute: (id: string, command: string) => Promise<CommandResult>, id: string, run: string) {
  const command = helloWebsiteCommand(run)
  const key = `${id}:${run}`
  const pending = builds.get(key)
  if (pending) return pending
  // React remounts/repeated clicks share one dispatch. The guest's lock and
  // response check also prevent duplicate servers across windows and retries.
  const build = Promise.resolve().then(() => execute(id, command)).then(result => {
    if (result.exitCode !== 0 || result.stdout.trim().split(/\r?\n/).at(-1) !== marker(run)) {
      const detail = (result.stderr.trim() || (result.exitCode !== 0 ? result.stdout.trim() : "")).slice(-700)
      throw new Error(`Could not build your website. Keep Internet access connected and retry.${detail ? ` ${detail}` : ""}`)
    }
  }).finally(() => builds.delete(key))
  builds.set(key, build)
  return build
}
