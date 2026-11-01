#!/usr/bin/env python3
"""Headless integrity tests for the GitHub-built native reader artifact runner."""
import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest
import warnings
import zipfile


SCRIPT = Path(__file__).with_name("actions-native-reader.py")
SPEC = importlib.util.spec_from_file_location("actions_native_reader", SCRIPT)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


SHA_A = "a" * 40
SHA_B = "b" * 40
SHA_C = "c" * 40
RUN_ID = 1234
ATTEMPT = 2
VERSION = "0.6.16"
REPO = "iarnvidr/hrungnir"
APP = "Hrungnir Test.app"


def receipt(archive: Path) -> dict:
    return runner.make_receipt(repo=REPO, run_id=RUN_ID, run_attempt=ATTEMPT,
                               source_head=SHA_A, checkout_sha=SHA_B,
                               version=VERSION, archive=archive)


def successful_pr_run() -> dict:
    return {
        "id": RUN_ID,
        "run_attempt": ATTEMPT,
        "status": "completed",
        "conclusion": "success",
        "path": ".github/workflows/macos.yml@refs/pull/19/merge",
        "repository": {"full_name": REPO},
        "event": "pull_request",
        "head_sha": SHA_A,
        "pull_requests": [{"number": 19, "head": {"sha": SHA_A},
                            "base": {"sha": SHA_C}}],
    }


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.archive = self.root / f"Hrungnir-Test-{VERSION}-macos-arm64.zip"
        self.archive.write_bytes(b"test archive")

    def tearDown(self):
        self.temp.cleanup()

    def test_receipt_binds_archive_and_provenance(self):
        value = receipt(self.archive)
        checked = runner.validate_receipt(value, repo=REPO, run_id=RUN_ID,
                                          run_attempt=ATTEMPT, version=VERSION)
        runner.verify_archive(self.archive, checked)
        self.assertEqual(value["app_identifier"], runner.APP_ID)
        self.assertEqual(value["target"], "darwin-aarch64")

    def test_receipt_rejects_wrong_repo_attempt_identity_target_and_hash(self):
        valid = receipt(self.archive)
        cases = [
            ({**valid, "repo": "attacker/repo"}, {"repo": REPO}),
            ({**valid, "run_attempt": ATTEMPT + 1}, {"run_attempt": ATTEMPT}),
            ({**valid, "app_identifier": "io.github.iarnvidr.hrungnir"}, {}),
            ({**valid, "target": "darwin-x86_64"}, {}),
            ({**valid, "archive": {**valid["archive"], "sha256": "0" * 64}}, {}),
            ({**valid, "token": "must-not-be-in-receipt"}, {}),
        ]
        for bad, overrides in cases:
            with self.subTest(bad=bad):
                kwargs = {"repo": REPO, "run_id": RUN_ID,
                          "run_attempt": ATTEMPT, "version": VERSION}
                kwargs.update(overrides)
                with self.assertRaises(runner.ValidationError):
                    checked = runner.validate_receipt(bad, **kwargs)
                    runner.verify_archive(self.archive, checked)

    def test_pr_provenance_binds_checkout_and_current_source_head(self):
        value = receipt(self.archive)
        run = successful_pr_run()
        runner.validate_run(run, repo=REPO, run_id=RUN_ID, run_attempt=ATTEMPT)
        checkout = {"sha": SHA_B, "parents": [{"sha": SHA_A}, {"sha": SHA_C}]}
        runner.validate_provenance(
            value, run, current_head=SHA_A,
            current_pr={"number": 19, "headRefOid": SHA_A, "baseRefOid": SHA_C},
            checkout_commit=checkout)
        mismatches = [
            (SHA_B, {"number": 19, "headRefOid": SHA_A, "baseRefOid": SHA_C}, checkout),
            (SHA_A, {"number": 20, "headRefOid": SHA_A, "baseRefOid": SHA_C}, checkout),
            (SHA_A, {"number": 19, "headRefOid": SHA_B, "baseRefOid": SHA_C}, checkout),
            (SHA_A, {"number": 19, "headRefOid": SHA_A, "baseRefOid": SHA_C}, None),
            (SHA_A, {"number": 19, "headRefOid": SHA_A, "baseRefOid": SHA_B}, checkout),
        ]
        for head, pr, merge in mismatches:
            with self.subTest(head=head, pr=pr):
                with self.assertRaises(runner.ValidationError):
                    runner.validate_provenance(value, run, current_head=head,
                                               current_pr=pr, checkout_commit=merge)

    def test_non_pr_provenance_requires_exact_local_checkout(self):
        value = receipt(self.archive)
        value["source_head"] = SHA_B
        run = successful_pr_run()
        run["event"] = "workflow_dispatch"
        run["head_sha"] = SHA_B
        run["pull_requests"] = []
        runner.validate_provenance(value, run, current_head=SHA_B, current_pr=None)
        with self.assertRaises(runner.ValidationError):
            runner.validate_provenance(value, run, current_head=SHA_A, current_pr=None)

    def test_run_rejects_untrusted_workflow_and_artifact_origin(self):
        run = successful_pr_run()
        run["path"] = ".github/workflows/release.yml@refs/heads/main"
        with self.assertRaises(runner.ValidationError):
            runner.validate_run(run, repo=REPO, run_id=RUN_ID, run_attempt=ATTEMPT)
        with self.assertRaises(runner.ValidationError):
            runner.validate_artifact({"id": 1, "size_in_bytes": 20, "name": "lookalike", "workflow_run": {"id": RUN_ID}},
                                     expected_name=f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                     run_id=RUN_ID)
        with self.assertRaises(runner.ValidationError):
            runner.validate_artifact({"id": 1, "size_in_bytes": 20,
                                      "name": f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                      "workflow_run": {"id": RUN_ID + 1}},
                                     expected_name=f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                     run_id=RUN_ID)
        runner.validate_artifact({"id": 1, "size_in_bytes": 20,
                                  "name": f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                  "workflow_run": {"id": RUN_ID, "head_sha": SHA_B}},
                                 expected_name=f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                 run_id=RUN_ID, checkout_sha=SHA_B)
        with self.assertRaises(runner.ValidationError):
            runner.validate_artifact({"id": 1, "size_in_bytes": 20,
                                      "name": f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                      "workflow_run": {"id": RUN_ID, "head_sha": SHA_A}},
                                     expected_name=f"hrungnir-native-reader-test-{RUN_ID}-{ATTEMPT}",
                                     run_id=RUN_ID, checkout_sha=SHA_B)

    def test_zip_rejects_traversal_symlink_duplicates_and_wrong_bundle(self):
        valid_path = self.root / "valid.zip"
        with zipfile.ZipFile(valid_path, "w") as archive:
            archive.writestr(f"{APP}/Contents/Info.plist", b"plist")
            info = zipfile.ZipInfo(f"{APP}/Contents/MacOS/offline-reader-desktop")
            info.external_attr = (0o100755 << 16)
            archive.writestr(info, b"executable")
        names = runner.validate_zip(valid_path, expected_app=APP)
        self.assertEqual(len(names), 2)

        unsafe_archives = {
            "traversal": ["../outside"],
            "normalized-traversal": [f"{APP}/Contents/../outside"],
            "wrong-root": ["Other.app/Contents/Info.plist"],
            "duplicate": [f"{APP}/Contents/Info.plist", f"{APP}/Contents/Info.plist"],
        }
        for label, members in unsafe_archives.items():
            path = self.root / f"{label}.zip"
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(path, "w") as archive:
                    for name in members:
                        archive.writestr(name, b"x")
            with self.subTest(label=label), self.assertRaises(runner.ValidationError):
                runner.validate_zip(path, expected_app=APP)

        symlink_path = self.root / "symlink.zip"
        with zipfile.ZipFile(symlink_path, "w") as archive:
            info = zipfile.ZipInfo(f"{APP}/Contents/link")
            info.external_attr = (0o120777 << 16)
            archive.writestr(info, "../../outside")
        with self.assertRaises(runner.ValidationError):
            runner.validate_zip(symlink_path, expected_app=APP)

    def test_downloaded_github_artifact_extracts_only_named_receipt_and_app_zip(self):
        artifact_zip = self.root / "github-artifact.zip"
        archive_name = f"Hrungnir-Test-{VERSION}-macos-arm64.zip"
        with zipfile.ZipFile(artifact_zip, "w") as archive:
            archive.writestr("test-receipt.json", b"{}")
            archive.writestr(archive_name, b"bundle")
        destination = self.root / "download"
        destination.mkdir()
        runner.extract_downloaded_artifact(
            artifact_zip, destination,
            allowed_names={"test-receipt.json", archive_name})
        self.assertEqual((destination / "test-receipt.json").read_bytes(), b"{}")
        self.assertEqual((destination / archive_name).read_bytes(), b"bundle")

        bad = self.root / "github-artifact-unsafe.zip"
        with zipfile.ZipFile(bad, "w") as archive:
            archive.writestr("../outside", b"no")
        with self.assertRaises(runner.ValidationError):
            runner.extract_downloaded_artifact(
                bad, destination,
                allowed_names={"test-receipt.json", archive_name})

    def test_bundle_requires_test_identity_version_and_executable(self):
        app = self.root / APP
        executable = app / "Contents" / "MacOS" / "offline-reader-desktop"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"fixture")
        executable.chmod(0o755)
        info = {
            "CFBundleIdentifier": runner.APP_ID,
            "CFBundleShortVersionString": VERSION,
            "CFBundleVersion": VERSION,
            "CFBundleExecutable": "offline-reader-desktop",
            "LSUIElement": True,
        }
        info_path = app / "Contents" / "Info.plist"
        info_path.write_bytes(plistlib.dumps(info))
        self.assertEqual(runner.validate_bundle(app, version=VERSION), executable)
        info["CFBundleIdentifier"] = "io.github.iarnvidr.hrungnir"
        info_path.write_bytes(plistlib.dumps(info))
        with self.assertRaises(runner.ValidationError):
            runner.validate_bundle(app, version=VERSION)
        info["CFBundleIdentifier"] = runner.APP_ID
        info["CFBundleVersion"] = "0.0.0"
        info_path.write_bytes(plistlib.dumps(info))
        with self.assertRaises(runner.ValidationError):
            runner.validate_bundle(app, version=VERSION)
        info["CFBundleVersion"] = VERSION
        info["LSUIElement"] = False
        info_path.write_bytes(plistlib.dumps(info))
        with self.assertRaises(runner.ValidationError):
            runner.validate_bundle(app, version=VERSION)
        info["LSUIElement"] = True
        info["CFBundleURLTypes"] = [{"CFBundleURLSchemes": ["hrungnir"]}]
        info_path.write_bytes(plistlib.dumps(info))
        with self.assertRaises(runner.ValidationError):
            runner.validate_bundle(app, version=VERSION)


if __name__ == "__main__":
    unittest.main()
