#!/usr/bin/env python3
"""Portable security-policy guards; synthetic packaging is not a Mac launch test."""
import json
import os
import pathlib
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]


def assert_scoped_policy(test, info):
    ats = info["NSAppTransportSecurity"]
    test.assertEqual(set(ats), {"NSAllowsLocalNetworking", "NSExceptionDomains"})
    test.assertIs(ats["NSAllowsLocalNetworking"], True)
    test.assertEqual(set(ats["NSExceptionDomains"]), {"www.reddit.com"})
    exception = ats["NSExceptionDomains"]["www.reddit.com"]
    test.assertEqual(
        set(exception),
        {"NSIncludesSubdomains", "NSExceptionAllowsInsecureHTTPLoads"},
    )
    test.assertIs(exception["NSIncludesSubdomains"], False)
    test.assertIs(exception["NSExceptionAllowsInsecureHTTPLoads"], True)


class MacPolicyTests(unittest.TestCase):
    def test_legacy_build_entries_refuse_local_compilation(self):
        environment = os.environ.copy()
        environment.pop("GITHUB_ACTIONS", None)
        for shell, script in [("sh", "scripts/dev.sh"), ("bash", "scripts/build-macos.sh")]:
            with self.subTest(script=script):
                result = subprocess.run([shell, str(ROOT / script)], env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode, 2)
                self.assertIn("Local compilation is disabled", result.stderr)

    def test_shared_policy_allows_custom_trust_for_only_the_exact_source_host(self):
        info = plistlib.loads((ROOT / "src-tauri/Info.plist").read_bytes())
        assert_scoped_policy(self, info)

    def test_packaged_app_preserves_policy_identity_and_version(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        # The public app uses its own cache namespace.
        self.assertEqual(config["identifier"], "io.github.iarnvidr.hrungnir")
        self.assertEqual(config["productName"], "Hrungnir")
        shared = plistlib.loads((ROOT / "src-tauri/Info.plist").read_bytes())
        with tempfile.TemporaryDirectory(prefix="hrungnir-package-policy-") as directory:
            staging = pathlib.Path(directory)
            script = staging / "scripts/package-macos.py"
            script.parent.mkdir()
            shutil.copy2(ROOT / "scripts/package-macos.py", script)
            for source in ["src-tauri/tauri.conf.json", "src-tauri/Info.plist"]:
                destination = staging / source
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / source, destination)
            # These inert fixtures exercise bundle assembly without running or
            # pretending to validate an executable, icon, helper or signature.
            fixtures = {
                "src-tauri/target/aarch64-apple-darwin/release/offline-reader-desktop": struct.pack("<II", 0xFEEDFACF, 0x0100000C),
                "src-tauri/icons/icon.icns": b"synthetic-icon",
                "var/media-tools/yt-dlp": b"synthetic-downloader",
                "var/media-tools/ffmpeg": b"synthetic-encoder",
            }
            for name, contents in fixtures.items():
                path = staging / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(contents)
            subprocess.run([sys.executable, str(script)], check=True, capture_output=True, text=True)
            bundle = staging / "artifacts" / f"{config['productName']}.app"
            info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
            assert_scoped_policy(self, info)
            self.assertEqual(info["NSAppTransportSecurity"], shared["NSAppTransportSecurity"])
            self.assertEqual(info["CFBundleIdentifier"], config["identifier"])
            self.assertEqual(info["CFBundleDisplayName"], config["productName"])
            self.assertEqual(info["CFBundleName"], config["productName"])
            self.assertEqual(info["CFBundleShortVersionString"], config["version"])
            self.assertEqual(info["CFBundleVersion"], config["version"])
            self.assertEqual(info["CFBundleURLTypes"], shared["CFBundleURLTypes"])
            self.assertEqual(info["CFBundleURLTypes"][0]["CFBundleURLSchemes"], ["hrungnir"])
            self.assertEqual(info["CFBundleURLTypes"][0]["CFBundleURLName"], "io.github.iarnvidr.hrungnir.open")
            self.assertEqual(info["LSMinimumSystemVersion"], config["bundle"]["macOS"]["minimumSystemVersion"])
            self.assertTrue((bundle / "Contents/MacOS" / info["CFBundleExecutable"]).is_file())

            shutil.copy2(ROOT / "src-tauri/tauri.test.conf.json", staging / "src-tauri/tauri.test.conf.json")
            test_binary = staging / "src-tauri/target/aarch64-apple-darwin/debug/offline-reader-desktop"
            test_binary.parent.mkdir(parents=True)
            test_binary.write_bytes(struct.pack("<II", 0xFEEDFACF, 0x0100000C))
            subprocess.run([sys.executable, str(script), "--test"], check=True, capture_output=True, text=True)
            test_bundle = staging / "artifacts/native-test/Hrungnir Test.app"
            test_info = plistlib.loads((test_bundle / "Contents/Info.plist").read_bytes())
            assert_scoped_policy(self, test_info)
            self.assertEqual(test_info["CFBundleIdentifier"], "io.github.iarnvidr.hrungnir.dev.test")
            self.assertEqual(test_info["CFBundleDisplayName"], "Hrungnir Test")
            self.assertEqual(test_info["CFBundleShortVersionString"], config["version"])
            self.assertTrue(test_info["LSUIElement"])
            self.assertNotIn("CFBundleURLTypes", test_info)
            self.assertTrue((test_bundle / "Contents/MacOS" / test_info["CFBundleExecutable"]).is_file())
            self.assertTrue(bundle.is_dir(), "Test packaging must preserve the normal release bundle")
            archive_script = staging / "scripts/archive-macos.py"
            shutil.copy2(ROOT / "scripts/archive-macos.py", archive_script)
            subprocess.run([sys.executable, str(archive_script), "--test"], check=True, capture_output=True, text=True)
            archive = staging / f"artifacts/native-test/Hrungnir-Test-{config['version']}-macos-arm64.zip"
            with zipfile.ZipFile(archive) as zipped:
                entry = zipped.getinfo("Hrungnir Test.app/Contents/MacOS/offline-reader-desktop")
                self.assertTrue((entry.external_attr >> 16) & 0o111)
                self.assertTrue(all(name.startswith("Hrungnir Test.app/") for name in zipped.namelist()))


if __name__ == "__main__":
    unittest.main()
