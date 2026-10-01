"""The systemd unit must be able to start on a fresh VM."""

import re
import unittest
from pathlib import Path

UNIT = Path(__file__).resolve().parent.parent / "skimasque-deploy.service"


class UnitTests(unittest.TestCase):
    def setUp(self):
        self.text = UNIT.read_text()

    def test_every_read_write_path_either_is_optional_or_is_created_before_the_sandbox(self):
        # Under ProtectSystem=strict, systemd refuses to start (226/NAMESPACE) when a
        # ReadWritePaths entry does not exist -- unless it is prefixed with `-`. A fresh
        # VM has neither /opt/skimasque nor /var/lib/skimasque-deploy, so the unit must
        # create them with a privileged (`+`) ExecStartPre before the sandbox is built.
        paths = re.search(r"^ReadWritePaths=(.*)$", self.text, re.M).group(1).split()
        pre = " ".join(re.findall(r"^ExecStartPre=\+(.*)$", self.text, re.M))
        self.assertTrue(paths)
        for path in paths:
            if path.startswith("-"):
                continue
            self.assertIn(path, pre, f"{path} must be created by ExecStartPre=+install -d")

    def test_the_directories_a_fresh_vm_lacks_are_optional_read_write_paths(self):
        # Regression: on a fresh VM (systemd 255) the unit failed with 226/NAMESPACE
        # before any ExecStartPre ran, because these two did not exist yet.
        paths = re.search(r"^ReadWritePaths=(.*)$", self.text, re.M).group(1).split()
        self.assertIn("-/opt/skimasque", paths)
        self.assertIn("-/var/lib/skimasque-deploy", paths)

    def test_the_agent_directories_have_safe_modes(self):
        self.assertRegex(self.text, r"ExecStartPre=\+/usr/bin/install -d -m 0755 [^\n]*/opt/skimasque/releases")
        self.assertRegex(self.text, r"ExecStartPre=\+/usr/bin/install -d -m 0700 /var/lib/skimasque-deploy")


if __name__ == "__main__":
    unittest.main()
