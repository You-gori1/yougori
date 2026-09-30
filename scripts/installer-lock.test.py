"""Exercise the installer lock in Linux without downloads or package changes."""
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "src-tauri/src/workspace/install-tools.sh"
PREFIX = SOURCE.read_text(encoding="utf-8").split('export PATH=', 1)[0]


class InstallerLockTests(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory(prefix="yougori-lock-home-")
        self.work = tempfile.TemporaryDirectory(prefix="yougori-install.")
        self.script = Path(self.work.name) / "install.sh"
        self.script.write_text("od_tool=lock-test\n" + PREFIX + "printf 'LOCK_ACQUIRED\\n'\n", encoding="utf-8")
        self.lock = Path(self.home.name) / ".local/share/yougori/install.lock"
        self.lock.mkdir(parents=True)
        self.env = dict(os.environ, HOME=self.home.name)
        self.children = []

    def tearDown(self):
        for child in self.children:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
            if child.stdout:
                child.stdout.close()
        self.work.cleanup()
        self.home.cleanup()

    def run_installer(self):
        return subprocess.run(["/bin/busybox", "sh", str(self.script)], env=self.env, text=True, capture_output=True, timeout=10)

    def assert_recovers(self):
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("LOCK_ACQUIRED", result.stdout)
        self.assertFalse(self.lock.exists())

    def test_reused_pid_does_not_block_installation(self):
        (self.lock / "pid").write_text(str(os.getpid()))
        self.assert_recovers()

    def test_old_boot_identity_does_not_block_installation(self):
        (self.lock / "pid").write_text(str(os.getpid()))
        (self.lock / "identity").write_text("previous-boot:100")
        self.assert_recovers()

    def test_old_empty_directory_is_recovered(self):
        old = time.time() - 60
        os.utime(self.lock, (old, old))
        self.assert_recovers()

    def test_fresh_empty_directory_is_not_removed(self):
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.lock.exists())

    def test_live_legacy_installer_is_not_interrupted(self):
        with tempfile.TemporaryDirectory(prefix="yougori-install.") as directory:
            legacy = Path(directory) / "install.sh"
            legacy.write_text("sleep 30\n")
            child = subprocess.Popen(["/bin/busybox", "sh", str(legacy)], start_new_session=True)
            self.children.append(child)
            (self.lock / "pid").write_text(str(child.pid))
            result = self.run_installer()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("still running", result.stderr)
            self.assertIsNone(child.poll())
            self.assertEqual((self.lock / "pid").read_text(), str(child.pid))

    def test_kernel_lock_keeps_concurrent_installer_out(self):
        self.lock.rmdir()
        with tempfile.TemporaryDirectory(prefix="yougori-install.") as directory:
            holder = Path(directory) / "install.sh"
            holder.write_text("od_tool=lock-test\n" + PREFIX + "printf 'READY\\n'\nsleep 30\n")
            child = subprocess.Popen(["/bin/busybox", "sh", str(holder)], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, start_new_session=True)
            self.children.append(child)
            self.assertEqual(child.stdout.readline().strip(), "READY")
            result = self.run_installer()
            self.assertNotEqual(result.returncode, 0)
            self.assertIsNone(child.poll())
            self.assertEqual((self.lock / "pid").read_text().strip(), str(child.pid))


if __name__ == "__main__":
    unittest.main()
