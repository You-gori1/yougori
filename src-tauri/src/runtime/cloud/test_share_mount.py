"""Linux/root integration test: cloud terminals can reach a selected folder from home."""
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import unittest


HERE = Path(__file__).resolve().parent
HELPER = HERE.parents[2] / 'resources/runtime/cloud/yougori-share-linux-amd64'


class MountedCloudFolder(unittest.TestCase):
    def test_new_terminal_starts_at_home_with_live_v1_folder_available(self):
        if os.geteuid() != 0 or not Path('/dev/fuse').exists():
            self.skipTest('Direct FUSE mount needs root and /dev/fuse')
        with tempfile.TemporaryDirectory(prefix='yougori-cloud-agent-') as home:
            env = dict(os.environ, HOME=home, SHELL='/bin/sh')
            source = (HERE / 'agent.py').read_bytes()
            bootstrap = 'import sys;code=sys.stdin.buffer.read(int(sys.stdin.buffer.readline()));exec(compile(code,"yougori-agent","exec"))'
            process = subprocess.Popen(['python3', '-u', '-c', bootstrap], env=env,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            process.stdin.write(str(len(source)).encode() + b'\n' + source)
            process.stdin.flush()
            responses = queue.Queue()
            writing = threading.Lock()

            def send(value):
                with writing:
                    process.stdin.write((json.dumps(value) + '\n').encode())
                    process.stdin.flush()

            def receive():
                for line in process.stdout:
                    message = json.loads(line)
                    if message.get('event') != 'files':
                        responses.put(message)
                        continue
                    raw = base64.b64decode(message['data'])
                    body = json.loads(raw.split(b'\r\n\r\n', 1)[1])
                    self.assertEqual(body['connectionId'], 'conn-test')
                    self.assertTrue(body['path'].startswith('_selected/0'))
                    relative = body['path'].removeprefix('_selected/0').strip('/')
                    if body['operation'] == 'stat':
                        result = {'info': {'name': relative, 'directory': not relative, 'size': 5 if relative else 0}}
                    elif body['operation'] == 'list':
                        result = {'entries': [{'name': 'hello.txt', 'directory': False, 'size': 5}]}
                    elif body['operation'] == 'read':
                        offset = body.get('offset', 0)
                        length = body.get('length', 65536)
                        result = {'data': base64.b64encode(b'hello'[offset:offset + length]).decode()}
                    else:
                        result = {'error': 'Not granted'}
                    payload = json.dumps(result).encode()
                    reply = b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %d\r\n\r\n' % len(payload) + payload
                    send({'requestId': message['requestId'], 'result': base64.b64encode(reply).decode()})

            thread = threading.Thread(target=receive, daemon=True)
            thread.start()

            def rpc(path, body):
                ident = str(time.monotonic_ns())
                send({'requestId': ident, 'path': path, 'body': body})
                while True:
                    message = responses.get(timeout=30)
                    if message.get('requestId') == ident:
                        if 'error' in message: raise AssertionError(message['error'])
                        return message['result']

            try:
                health = rpc('health', {})
                self.assertEqual(health['platform'], 'linux')
                binary = HELPER.read_bytes()
                rpc('share/helper', {'action': 'begin', 'checksum': hashlib.sha256(binary).hexdigest()})
                for offset in range(0, len(binary), 65536):
                    rpc('share/helper', {'action': 'chunk', 'data': base64.b64encode(binary[offset:offset + 65536]).decode()})
                rpc('share/helper', {'action': 'finish'})
                result = rpc('share/attach', {'connectionId': 'conn-test', 'index': 0, 'label': 'V1', 'readOnly': True})
                mount = Path(result['mountPath'])
                self.assertEqual(mount.parent, Path(home) / 'Yougori/shared')
                self.assertEqual(os.listdir(mount), ['hello.txt'])
                self.assertEqual((mount / 'hello.txt').read_bytes(), b'hello')
                output = subprocess.check_output(['/bin/sh', '-c', 'pwd; ls; cat V1/hello.txt'], cwd=mount.parent, timeout=10)
                self.assertIn(str(mount.parent).encode(), output)
                self.assertIn(b'V1', output)
                self.assertIn(b'hello', output)
                terminal = 'term-mounted-home'
                rpc('/v1/terminal/create', {'sessionId': terminal})
                command = 'printf "HOME_CWD_%s\\n" "$PWD"; printf "CONTENT_%s\\n" "$(cat "$HOME/Yougori/shared/V1/hello.txt")"\n'
                rpc('/v1/terminal/write', {'sessionId': terminal, 'data': base64.b64encode(command.encode()).decode()})
                text, offset = '', 0
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and ('HOME_CWD_' + home not in text or 'CONTENT_hello' not in text):
                    data = rpc('/v1/terminal/read', {'sessionId': terminal, 'offset': offset})
                    offset = data['offset']
                    text += base64.b64decode(data['data']).decode('utf-8', 'replace')
                    time.sleep(.05)
                self.assertIn('HOME_CWD_' + home, text)
                self.assertIn('CONTENT_hello', text)
                rpc('/v1/terminal/close', {'sessionId': terminal})
                rpc('share/detach', {'connectionId': 'conn-test', 'index': 0})
                self.assertFalse(os.path.ismount(mount))
            finally:
                process.stdin.close()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
                process.stdout.close()
                detail = process.stderr.read().decode('utf-8', 'replace')
                process.stderr.close()
                self.assertEqual(process.returncode, 0, detail)


if __name__ == '__main__':
    unittest.main()
