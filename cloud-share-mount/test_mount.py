"""Linux integration check for the bundled cloud mount against a fake private API."""
import base64
import http.server
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import threading
import unittest


class API(http.server.BaseHTTPRequestHandler):
    calls = []
    files = {'hello.txt': b'hello'}
    def log_message(self, *_args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.calls.append(request)
        if request['connectionId'] != 'conn-test' or not request['path'].startswith('_selected/0'):
            self.send_error(403)
            return
        relative = request['path'].removeprefix('_selected/0').strip('/')
        operation = request['operation']
        if operation == 'stat' and relative and relative not in self.files:
            body = b'{"error":"no such file or directory"}'
            self.send_response(403)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if operation == 'list':
            response = {'entries': [{'name': name, 'directory': False, 'size': len(data)} for name, data in self.files.items()]}
        elif operation == 'stat':
            response = {'info': {'name': relative, 'directory': not relative, 'size': len(self.files[relative]) if relative else 0}}
        elif operation == 'read':
            offset = request.get('offset', 0)
            length = request.get('length', 65536)
            response = {'data': base64.b64encode(self.files[relative][offset:offset + length]).decode('ascii')}
        elif operation == 'create':
            self.files[relative] = b''
            response = {'ok': True}
        elif operation == 'write':
            data = base64.b64decode(request['data'])
            old = self.files[relative]
            offset = request.get('offset', 0)
            self.files[relative] = old[:offset] + data + old[offset + len(data):]
            response = {'count': len(data)}
        elif operation == 'truncate':
            size = request['length']
            self.files[relative] = self.files[relative][:size].ljust(size, b'\0')
            response = {'ok': True}
        else:
            self.send_error(403)
            return
        body = json.dumps(response).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class MountedFolder(unittest.TestCase):
    def test_ls_and_cat_see_selected_folder(self):
        if os.geteuid() != 0 or not Path('/dev/fuse').exists():
            self.skipTest('Direct FUSE mount needs root and /dev/fuse')
        helper = Path(__file__).resolve().parents[1] / 'src-tauri/resources/runtime/cloud/yougori-share-linux-amd64'
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), API)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        with tempfile.TemporaryDirectory(prefix='yougori-cloud-mount-') as directory:
            mount = Path(directory) / 'V1'
            mount.mkdir()
            process = subprocess.Popen([str(helper), str(mount), str(server.server_port), 'conn-test', '0', 'false', '0', '0'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                readable, _, _ = select.select([process.stdout], [], [], 15)
                self.assertTrue(readable, 'helper did not start')
                self.assertEqual(process.stdout.readline().strip(), b'yougori-share-ready')
                self.assertEqual(os.listdir(mount), ['hello.txt'])
                self.assertFalse((mount / 'missing.txt').exists())
                try:
                    self.assertEqual((mount / 'hello.txt').read_bytes(), b'hello')
                except OSError as error:
                    self.fail(f'{error}; requests={API.calls!r}')
                (mount / 'new.txt').write_bytes(b'new')
                self.assertEqual(API.files['new.txt'], b'new')
            finally:
                if os.path.ismount(mount):
                    subprocess.run(['umount', '-l', str(mount)], check=True)
                process.kill()
                process.wait(timeout=5)
                process.stdout.close()
                process.stderr.close()
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    unittest.main()
