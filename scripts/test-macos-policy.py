#!/usr/bin/env python3
"""Portable security-policy guards; synthetic packaging is not a Mac launch test."""
import json
import pathlib
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest


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
    def test_shared_policy_allows_custom_trust_for_only_the_exact_source_host(self):
        info = plistlib.loads((ROOT / "src-tauri/Info.plist").read_bytes())
        assert_scoped_policy(self, info)

    def test_packaged_app_preserves_policy_identity_and_version(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        # Preserve the installed app's cache namespace while changing its branding.
        self.assertEqual(config["identifier"], "xyz.ginnungagaps.offline-reader-exploration")
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
            self.assertEqual(info["CFBundleURLTypes"][0]["CFBundleURLName"], "xyz.ginnungagaps.hrungnir.open")
            self.assertEqual(info["LSMinimumSystemVersion"], config["bundle"]["macOS"]["minimumSystemVersion"])
            self.assertTrue((bundle / "Contents/MacOS" / info["CFBundleExecutable"]).is_file())


if __name__ == "__main__":
    unittest.main()
