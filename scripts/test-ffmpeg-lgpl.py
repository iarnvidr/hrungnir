#!/usr/bin/env python3
"""Guards for the LGPL-only FFmpeg recipe; building it needs an Apple Silicon Mac."""
import pathlib
import platform
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import ffmpeg_lgpl as recipe  # noqa: E402

LGPL_BINARY = (
    b"\0" + " ".join(recipe.configure_args()).encode() + b"\0"
    b"libavcodec license: LGPL version 2.1 or later\0"
)
GPL_BINARY = (
    b"\0--enable-static --enable-gpl --enable-version3 --enable-libx264\0"
    b"libavcodec license: GPL version 3 or later\0"
)


class RecipeTests(unittest.TestCase):
    def write(self, data):
        import tempfile

        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = pathlib.Path(directory.name) / "ffmpeg"
        path.write_bytes(data)
        return path

    def test_configuration_is_lgpl_only(self):
        args = recipe.configure_args()
        self.assertIn("--disable-gpl", args)
        self.assertIn("--disable-nonfree", args)
        self.assertIn("--disable-autodetect", args)
        for flag in recipe.FORBIDDEN_IN_CONFIGURATION:
            self.assertNotIn(flag, args)
        for needed in ("--enable-videotoolbox", "--enable-securetransport", "--arch=arm64"):
            self.assertIn(needed, args)

    @unittest.skipIf(sys.platform == "darwin", "verify() executes the binary on macOS")
    def test_verify_accepts_lgpl_and_reports_the_configure_string(self):
        configuration = recipe.verify(self.write(LGPL_BINARY))
        self.assertEqual(configuration, " ".join(recipe.configure_args()))

    @unittest.skipIf(sys.platform == "darwin", "verify() executes the binary on macOS")
    def test_verify_rejects_gpl_builds(self):
        with self.assertRaises(recipe.LicenseError):
            recipe.verify(self.write(GPL_BINARY))

    def test_static_checks_reject_gpl_and_unknown_binaries(self):
        with self.assertRaises(recipe.LicenseError):
            recipe._check_flags(recipe.static_configuration(GPL_BINARY))
        self.assertIsNone(recipe.static_configuration(b"not ffmpeg"))
        recipe._check_flags(recipe.static_configuration(LGPL_BINARY))

    @unittest.skipIf(sys.platform == "darwin", "verify() executes the binary on macOS")
    def test_verify_requires_the_lgpl_marker(self):
        configuration_only = b"\0" + " ".join(recipe.configure_args()).encode() + b"\0"
        with self.assertRaises(recipe.LicenseError):
            recipe.verify(self.write(configuration_only))

    @unittest.skipIf(sys.platform == "darwin" and platform.machine() == "arm64", "can build here")
    def test_build_is_refused_off_apple_silicon(self):
        with self.assertRaises(recipe.BuildUnavailable):
            recipe.build(b"")


if __name__ == "__main__":
    unittest.main()
