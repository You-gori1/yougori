"""Offline checks: the generated shortcut never launches a real engine here."""
from pathlib import Path
import runpy
import unittest
from unittest.mock import patch, Mock

SCRIPT = Path(__file__).resolve().parents[1] / "cli/src/launcher/assets/python-yougori.py"


class ShortcutTests(unittest.TestCase):
    def test_arguments_working_directory_exit_status_and_repeated_interrupts(self):
        module = runpy.run_path(str(SCRIPT))
        process = Mock()
        process.wait.side_effect = [*[KeyboardInterrupt() for _ in range(10)], 7]
        with patch("shutil.which", return_value=str(SCRIPT.parent / "installed-cli")), \
                patch("subprocess.Popen", return_value=process) as start, \
                patch("sys.argv", ["yougori", "--change"]):
            self.assertEqual(module["main"](), 7)
        start.assert_called_once_with(
            [str(SCRIPT.parent / "installed-cli"), "launch", "--change"], cwd=SCRIPT.parent
        )

    def test_missing_install_or_self_resolution_never_recurses(self):
        module = runpy.run_path(str(SCRIPT))
        for resolved in (None, str(SCRIPT)):
            with patch("shutil.which", return_value=resolved), patch("subprocess.Popen") as start:
                self.assertEqual(module["main"](), 1)
                start.assert_not_called()


if __name__ == "__main__":
    unittest.main()
