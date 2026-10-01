"""Tests for skimasque_deploy. Run on Linux: python3 -m unittest discover -s tests -v"""

import base64
import hashlib
import http.client
import io
import json
import os
import stat
import subprocess
import tempfile
import sys
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import skimasque_deploy as sd  # noqa: E402


class FakeSystemd:
    def __init__(self):
        self.restarts = []
        self.active = True
        self.fail_next_restart = False

    def restart(self, unit):
        self.restarts.append(unit)
        if self.fail_next_restart:
            self.fail_next_restart = False
            raise subprocess.CalledProcessError(1, ["systemctl", "restart", unit])

    def is_active(self, unit):
        return self.active


class FakeSecrets:
    def __init__(self, values):
        self.values = values
        self.fail = False
        self.calls = 0

    def access(self, name):
        self.calls += 1
        if self.fail:
            raise urllib.error.URLError("network down")
        return self.values.get(name)


class Harness(unittest.TestCase):
    """A temporary root, a fake bucket directory, and a fake service manager."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        base = Path(self._tmp.name)
        self.bucket = base / "bucket"
        self.bucket.mkdir()
        self.root = base / "root"
        self.cfg = sd.Config(
            role="control",
            source=f"file://{self.bucket}",
            service="skimasque-control.service",
            binary="skimasque-control",
            health="active",
            root=self.root,
        )
        self.source = sd.FileSource(self.bucket)
        self.systemd = FakeSystemd()
        self.secrets_changed = False
        self.sync_calls = 0

    # -- helpers
    def promote(self, version, content=b"binary-" + b"x", sha=None):
        d = self.bucket / "control" / version
        d.mkdir(parents=True, exist_ok=True)
        (d / "skimasque-control").write_bytes(content)
        digest = sha if sha is not None else hashlib.sha256(content).hexdigest()
        (d / "skimasque-control.sha256").write_text(f"{digest}  skimasque-control\n")

    def point(self, version):
        ch = self.bucket / "channels"
        ch.mkdir(exist_ok=True)
        (ch / "control").write_text(version + "\n")

    def sync(self):
        self.sync_calls += 1
        return self.secrets_changed

    force_unhealthy = False

    def probe(self):
        if self.force_unhealthy:
            return False
        current = sd.current_version(self.cfg)
        if current is None:
            return False
        return b"BAD" not in (self.cfg.releases / current / "skimasque-control").read_bytes()

    def run_deploy(self):
        return sd.deploy(
            self.cfg,
            self.source,
            self.systemd,
            self.sync,
            self.probe,
            sleep=lambda s: None,
            clock=iter(range(0, 10_000, 30)).__next__,
        )


class DeployTests(Harness):
    def test_a_new_version_installs_and_restarts(self):
        self.promote("v1")
        self.point("v1")
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(sd.current_version(self.cfg), "v1")
        binary = self.cfg.current / "skimasque-control"
        self.assertEqual(binary.read_bytes(), b"binary-x")
        self.assertTrue(binary.stat().st_mode & stat.S_IXUSR)
        self.assertEqual(self.systemd.restarts, ["skimasque-control.service"])

    def test_the_current_symlink_is_relative_so_it_survives_a_root_move(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        self.assertEqual(os.readlink(self.cfg.current), "releases/v1")

    def test_an_unchanged_version_is_a_no_op(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(len(self.systemd.restarts), 1)

    def test_a_missing_pointer_does_nothing(self):
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(self.systemd.restarts, [])
        self.assertIsNone(sd.current_version(self.cfg))

    def test_a_checksum_mismatch_is_rejected_and_not_remembered_as_bad(self):
        self.promote("v1", sha="0" * 64)
        self.point("v1")
        self.assertEqual(self.run_deploy(), 1)
        self.assertIsNone(sd.current_version(self.cfg))
        self.assertEqual(self.systemd.restarts, [])
        self.assertEqual(sd.read_bad(self.cfg), set())

    def test_a_missing_artifact_is_an_error_not_a_crash(self):
        self.point("v9")
        self.assertEqual(self.run_deploy(), 1)
        self.assertIsNone(sd.current_version(self.cfg))

    def test_a_path_like_pointer_is_refused(self):
        for bad in ["../etc", "a/b", "", ".hidden", "v1 v2", "x" * 65]:
            self.point(bad)
            self.assertEqual(self.run_deploy(), 1, repr(bad))
            self.assertIsNone(sd.current_version(self.cfg))

    def test_an_unhealthy_version_rolls_back_is_marked_bad_and_not_retried(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        self.promote("v2", content=b"BAD-build")
        self.point("v2")
        self.assertEqual(self.run_deploy(), 1)
        self.assertEqual(sd.current_version(self.cfg), "v1")
        self.assertEqual(sd.read_bad(self.cfg), {"v2"})
        restarts = len(self.systemd.restarts)
        self.assertEqual(restarts, 3)  # install v1, try v2, roll back to v1

        # The channel still says v2: the agent leaves it alone.
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(len(self.systemd.restarts), restarts)

    def test_a_first_install_that_is_unhealthy_is_marked_bad(self):
        self.promote("v1", content=b"BAD-build")
        self.point("v1")
        self.assertEqual(self.run_deploy(), 1)
        self.assertEqual(sd.read_bad(self.cfg), {"v1"})

    def test_moving_the_channel_back_to_the_previous_version_redeploys_it(self):
        for v in ("v1", "v2"):
            self.promote(v, content=f"binary-{v}".encode())
            self.point(v)
            self.assertEqual(self.run_deploy(), 0)
        self.point("v1")
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(sd.current_version(self.cfg), "v1")
        self.assertEqual(
            (self.cfg.current / "skimasque-control").read_bytes(), b"binary-v1"
        )

    def test_pruning_keeps_the_two_newest_releases_and_the_current_one(self):
        for i, v in enumerate(["v1", "v2", "v3", "v4"]):
            self.promote(v, content=f"binary-{v}".encode())
            self.point(v)
            self.assertEqual(self.run_deploy(), 0)
            os.utime(self.cfg.releases / v, (1000 + i, 1000 + i))
        left = sorted(d.name for d in self.cfg.releases.iterdir())
        self.assertEqual(left, ["v3", "v4"])

    def test_pruning_never_removes_the_release_in_use_even_if_it_is_the_oldest(self):
        for v in ("v1", "v2"):
            self.promote(v)
            self.point(v)
            self.run_deploy()
        (self.cfg.releases / "v3").mkdir()
        sd.point_current_at(self.cfg, "v1")
        for i, v in enumerate(["v1", "v2", "v3"]):
            os.utime(self.cfg.releases / v, (1000 + i, 1000 + i))
        sd.prune(self.cfg)
        self.assertEqual(sorted(d.name for d in self.cfg.releases.iterdir()), ["v1", "v2", "v3"])


class HealthTimeoutTests(Harness):
    def test_the_health_timeout_is_configurable(self):
        def probes_until_giving_up(timeout):
            self.cfg = sd.Config(
                role="control",
                source=f"file://{self.bucket}",
                service="skimasque-control.service",
                binary="skimasque-control",
                health="active",
                root=self.root / f"t{timeout}",
                health_timeout=timeout,
            )
            self.promote("v1", content=b"BAD-build")
            self.point("v1")
            calls = []
            sd.deploy(
                self.cfg,
                self.source,
                self.systemd,
                lambda: False,
                lambda: calls.append(1) or False,
                sleep=lambda s: None,
                clock=iter(range(0, 100_000, 30)).__next__,
            )
            return len(calls)

        self.assertGreater(probes_until_giving_up(300), probes_until_giving_up(60))


class SecretSyncInDeployTests(Harness):
    def test_secrets_are_synced_before_install_and_a_change_restarts_an_unchanged_version(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        self.assertEqual(self.sync_calls, 1)
        self.assertEqual(len(self.systemd.restarts), 1)
        self.secrets_changed = True
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(len(self.systemd.restarts), 2)

    def test_a_required_secret_that_is_missing_aborts_before_anything_changes(self):
        self.promote("v1")
        self.point("v1")

        def broken_sync():
            raise sd.DeployError("missing required secret(s): admin-token")

        with self.assertRaises(sd.DeployError):
            sd.deploy(self.cfg, self.source, self.systemd, broken_sync, self.probe)
        self.assertIsNone(sd.current_version(self.cfg))
        self.assertEqual(self.systemd.restarts, [])
        self.assertEqual(sd.read_bad(self.cfg), set())


class SecretsTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.path = Path(self._tmp.name) / "etc" / "control-secrets.env"
        self.specs = sd.parse_secret_map(
            "admin-token=SKIMASQUE_CONTROL_ADMIN_TOKEN,"
            "stripe-key=SKIMASQUE_CONTROL_STRIPE_SECRET_KEY?"
        )

    def test_parse_marks_optional_secrets(self):
        self.assertEqual(
            self.specs,
            [
                sd.SecretSpec("admin-token", "SKIMASQUE_CONTROL_ADMIN_TOKEN", False),
                sd.SecretSpec("stripe-key", "SKIMASQUE_CONTROL_STRIPE_SECRET_KEY", True),
            ],
        )
        for bad in ["nomapping", "a=lower", "=X", "a=B C"]:
            with self.assertRaises(sd.DeployError, msg=bad):
                sd.parse_secret_map(bad)
        self.assertEqual(sd.parse_secret_map(""), [])

    def test_the_file_is_mode_0600_and_an_absent_optional_secret_is_skipped(self):
        secrets = FakeSecrets({"admin-token": "abc123\n"})
        self.assertTrue(sd.sync_secrets(self.specs, secrets, self.path))
        self.assertEqual(self.path.read_text(), "SKIMASQUE_CONTROL_ADMIN_TOKEN=abc123\n")
        self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o600)

    def test_an_unchanged_result_reports_no_change(self):
        secrets = FakeSecrets({"admin-token": "abc123"})
        sd.sync_secrets(self.specs, secrets, self.path)
        self.assertFalse(sd.sync_secrets(self.specs, secrets, self.path))

    def test_optional_secrets_are_included_when_present(self):
        secrets = FakeSecrets({"admin-token": "a", "stripe-key": "sk_test_1"})
        sd.sync_secrets(self.specs, secrets, self.path)
        self.assertIn("SKIMASQUE_CONTROL_STRIPE_SECRET_KEY=sk_test_1\n", self.path.read_text())

    def test_a_missing_required_secret_raises_and_leaves_the_file_alone(self):
        self.path.parent.mkdir(parents=True)
        self.path.write_text("KEEP=me\n")
        with self.assertRaisesRegex(sd.DeployError, "admin-token"):
            sd.sync_secrets(self.specs, FakeSecrets({}), self.path)
        self.assertEqual(self.path.read_text(), "KEEP=me\n")

    def test_a_failed_fetch_never_truncates_an_existing_file(self):
        secrets = FakeSecrets({"admin-token": "abc123"})
        sd.sync_secrets(self.specs, secrets, self.path)
        secrets.fail = True
        with self.assertRaises(urllib.error.URLError):
            sd.sync_secrets(self.specs, secrets, self.path)
        self.assertEqual(self.path.read_text(), "SKIMASQUE_CONTROL_ADMIN_TOKEN=abc123\n")

    def test_values_that_could_break_an_env_file_are_refused(self):
        for value in ["a b", 'a"b', "a$b", "a\nB=c", "", "  "]:
            with self.assertRaises(sd.DeployError, msg=repr(value)):
                sd.sync_secrets(self.specs, FakeSecrets({"admin-token": value}), self.path)
        self.assertFalse(self.path.exists())

    def test_secret_manager_decodes_the_payload_and_maps_404_to_none(self):
        def ok(req, timeout):
            self.assertIn("/projects/p1/secrets/admin-token/versions/latest:access", req.full_url)
            self.assertEqual(req.get_header("Authorization"), "Bearer tok")
            body = json.dumps({"payload": {"data": base64.b64encode(b"s3cret").decode()}})
            return io.BytesIO(body.encode())

        manager = sd.SecretManager("p1", token=lambda: "tok")
        with mock.patch("urllib.request.urlopen", side_effect=ok):
            self.assertEqual(manager.access("admin-token"), "s3cret")

        def not_found(req, timeout):
            raise urllib.error.HTTPError(req.full_url, 404, "nf", {}, None)

        with mock.patch("urllib.request.urlopen", side_effect=not_found):
            self.assertIsNone(manager.access("admin-token"))

        def boom(req, timeout):
            raise urllib.error.HTTPError(req.full_url, 500, "boom", {}, None)

        with mock.patch("urllib.request.urlopen", side_effect=boom):
            with self.assertRaises(urllib.error.HTTPError):
                manager.access("admin-token")


class SourceTests(unittest.TestCase):
    def test_gcs_reads_an_encoded_object_with_a_bearer_token(self):
        seen = {}

        def fake(req, timeout):
            seen["url"] = req.full_url
            seen["auth"] = req.get_header("Authorization")
            return io.BytesIO(b"payload")

        source = sd.GcsSource("my-bucket", token=lambda: "tok")
        with mock.patch("urllib.request.urlopen", side_effect=fake):
            self.assertEqual(source.read("control/v1/skimasque-control"), b"payload")
        self.assertEqual(
            seen["url"],
            "https://storage.googleapis.com/storage/v1/b/my-bucket/o/"
            "control%2Fv1%2Fskimasque-control?alt=media",
        )
        self.assertEqual(seen["auth"], "Bearer tok")

    def test_gcs_maps_404_to_not_found(self):
        def fake(req, timeout):
            raise urllib.error.HTTPError(req.full_url, 404, "nf", {}, None)

        with mock.patch("urllib.request.urlopen", side_effect=fake):
            with self.assertRaises(sd.NotFound):
                sd.GcsSource("b", token=lambda: "t").read("x")

    def test_parse_source(self):
        self.assertIsInstance(sd.parse_source("gs://bucket/"), sd.GcsSource)
        self.assertEqual(sd.parse_source("gs://bucket/").bucket, "bucket")
        self.assertIsInstance(sd.parse_source("file:///tmp/x"), sd.FileSource)
        with self.assertRaises(sd.DeployError):
            sd.parse_source("s3://nope")


class HealthTests(unittest.TestCase):
    def test_wait_healthy_polls_until_healthy_or_the_deadline(self):
        results = iter([False, False, True])
        clock = iter(range(0, 1000, 5))
        self.assertTrue(
            sd.wait_healthy(lambda: next(results), 60, 2, sleep=lambda s: None, clock=lambda: next(clock))
        )
        clock = iter(range(0, 1000, 30))
        self.assertFalse(
            sd.wait_healthy(lambda: False, 60, 2, sleep=lambda s: None, clock=lambda: next(clock))
        )

    def test_make_probe_understands_its_three_forms_and_refuses_others(self):
        systemd = FakeSystemd()
        self.assertTrue(sd.make_probe("active", "u", systemd)())
        systemd.active = False
        self.assertFalse(sd.make_probe("active", "u", systemd)())
        self.assertTrue(callable(sd.make_probe("http://127.0.0.1:9090/healthz", "u", systemd)))
        self.assertTrue(callable(sd.make_probe("https-local://control.example.com/healthz", "u", systemd)))
        with self.assertRaises(sd.DeployError):
            sd.make_probe("ftp://nope", "u", systemd)

    def test_the_http_probe_is_false_when_nothing_is_listening(self):
        self.assertFalse(sd.make_probe("http://127.0.0.1:1/healthz", "u", FakeSystemd())())


class ConfigTests(unittest.TestCase):
    ENV = {
        "SKIMASQUE_ROLE": "control",
        "SKIMASQUE_RELEASE_SOURCE": "gs://b",
        "SKIMASQUE_SERVICE": "skimasque-control.service",
        "SKIMASQUE_BINARY": "skimasque-control",
        "SKIMASQUE_HEALTH": "active",
    }

    def test_from_env_reads_the_required_settings(self):
        cfg = sd.Config.from_env(self.ENV)
        self.assertEqual((cfg.role, cfg.source, cfg.root), ("control", "gs://b", Path("/")))

    def test_each_required_setting_is_enforced(self):
        for name in self.ENV:
            env = {k: v for k, v in self.ENV.items() if k != name}
            with self.assertRaisesRegex(sd.DeployError, name):
                sd.Config.from_env(env)

    def test_the_health_timeout_defaults_and_is_validated(self):
        self.assertEqual(sd.Config.from_env(self.ENV).health_timeout, 60.0)
        self.assertEqual(
            sd.Config.from_env({**self.ENV, "SKIMASQUE_HEALTH_TIMEOUT": "180"}).health_timeout,
            180.0,
        )
        for bad in ["soon", "0", "-5"]:
            with self.assertRaises(sd.DeployError, msg=bad):
                sd.Config.from_env({**self.ENV, "SKIMASQUE_HEALTH_TIMEOUT": bad})

    def test_a_secret_map_needs_its_file_and_project(self):
        with self.assertRaises(sd.DeployError):
            sd.Config.from_env({**self.ENV, "SKIMASQUE_SECRET_MAP": "a=B"})
        cfg = sd.Config.from_env(
            {
                **self.ENV,
                "SKIMASQUE_SECRET_MAP": "a=B",
                "SKIMASQUE_SECRETS_FILE": "/etc/skimasque/x.env",
                "SKIMASQUE_GCP_PROJECT": "p",
            }
        )
        self.assertEqual(cfg.gcp_project, "p")


class MainTests(Harness):
    def test_main_runs_end_to_end_against_a_file_source(self):
        self.promote("v1")
        self.point("v1")
        env = {
            "SKIMASQUE_ROLE": "control",
            "SKIMASQUE_RELEASE_SOURCE": f"file://{self.bucket}",
            "SKIMASQUE_SERVICE": "skimasque-control.service",
            "SKIMASQUE_BINARY": "skimasque-control",
            "SKIMASQUE_HEALTH": "active",
            "SKIMASQUE_ROOT": str(self.root),
        }
        with mock.patch.object(sd, "Systemd", return_value=self.systemd):
            self.assertEqual(sd.main(env), 0)
        self.assertEqual(sd.current_version(self.cfg), "v1")
        self.assertEqual(self.systemd.restarts, ["skimasque-control.service"])

    def test_main_reports_a_config_error_as_exit_1(self):
        self.assertEqual(sd.main({}), 1)


class ReviewFixTests(Harness):
    def test_a_release_directory_is_world_traversable_so_a_dynamic_user_can_exec_it(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        release = self.cfg.releases / "v1"
        self.assertEqual(stat.S_IMODE(release.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((release / "skimasque-control").stat().st_mode), 0o755)

    def test_a_release_that_was_installed_but_never_verified_is_checked_on_the_next_run(self):
        self.promote("v1")
        self.point("v1")
        self.systemd.fail_next_restart = True  # interrupted right after the symlink swap
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_deploy()
        self.assertEqual(sd.current_version(self.cfg), "v1")

        self.assertEqual(self.run_deploy(), 0)  # the next tick verifies it
        self.assertEqual(sd.read_state(self.cfg, "verified"), "v1")
        self.assertEqual(len(self.systemd.restarts), 1)  # no extra restart
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(len(self.systemd.restarts), 1)

    def test_an_unverified_release_that_turns_out_unhealthy_is_rolled_back(self):
        self.promote("v1")
        self.point("v1")
        self.run_deploy()
        self.promote("v2", content=b"BAD-build")
        self.point("v2")
        self.systemd.fail_next_restart = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_deploy()
        self.assertEqual(sd.current_version(self.cfg), "v2")

        self.assertEqual(self.run_deploy(), 1)
        self.assertEqual(sd.current_version(self.cfg), "v1")
        self.assertEqual(sd.read_bad(self.cfg), {"v2"})

    def test_a_first_install_that_was_unhealthy_recovers_once_the_cause_is_fixed(self):
        self.promote("v1")
        self.point("v1")
        self.force_unhealthy = True  # e.g. DNS had not propagated for ACME yet
        self.assertEqual(self.run_deploy(), 1)
        self.assertEqual(sd.read_bad(self.cfg), {"v1"})
        restarts = len(self.systemd.restarts)

        self.force_unhealthy = False
        self.assertEqual(self.run_deploy(), 0)
        self.assertEqual(sd.read_bad(self.cfg), set(), "a release that verifies healthy is no longer bad")
        self.assertEqual(len(self.systemd.restarts), restarts, "re-verifying must not restart the service")

    def test_a_version_with_a_trailing_newline_inside_the_pointer_is_still_refused(self):
        self.point("v1\nv2")
        self.assertEqual(self.run_deploy(), 1)


class DestroyedSecretTests(unittest.TestCase):
    def test_a_destroyed_or_disabled_secret_version_reads_as_absent(self):
        def precondition_failed(req, timeout):
            body = io.BytesIO(b'{"error": {"code": 400, "status": "FAILED_PRECONDITION"}}')
            raise urllib.error.HTTPError(req.full_url, 400, "bad", {}, body)

        manager = sd.SecretManager("p1", token=lambda: "tok")
        with mock.patch("urllib.request.urlopen", side_effect=precondition_failed):
            self.assertIsNone(manager.access("registration-token"))

    def test_other_400s_are_still_errors(self):
        def bad_request(req, timeout):
            raise urllib.error.HTTPError(req.full_url, 400, "bad", {}, io.BytesIO(b'{"error": {"status": "INVALID_ARGUMENT"}}'))

        manager = sd.SecretManager("p1", token=lambda: "tok")
        with mock.patch("urllib.request.urlopen", side_effect=bad_request):
            with self.assertRaises(urllib.error.HTTPError):
                manager.access("x")


class MainErrorTests(Harness):
    def test_malformed_responses_from_google_are_a_clean_exit_1_not_a_traceback(self):
        env = {
            "SKIMASQUE_ROLE": "control",
            "SKIMASQUE_RELEASE_SOURCE": "gs://bucket",
            "SKIMASQUE_SERVICE": "skimasque-control.service",
            "SKIMASQUE_BINARY": "skimasque-control",
            "SKIMASQUE_HEALTH": "active",
            "SKIMASQUE_ROOT": str(self.root),
        }
        for error in (ValueError("bad json"), KeyError("access_token"), http.client.IncompleteRead(b"x")):
            with mock.patch.object(sd.GcsSource, "read", side_effect=error):
                self.assertEqual(sd.main(env), 1, repr(error))


if __name__ == "__main__":
    unittest.main()
