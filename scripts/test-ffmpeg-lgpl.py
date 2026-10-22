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
        for needed in ("--enable-videotoolbox", "--enable-securetransport", "--enable-libdav1d", "--arch=arm64"):
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

    def test_dav1d_is_pinned_to_a_release_tarball_with_a_sha256(self):
        self.assertRegex(recipe.DAV1D_SHA256, r"^[0-9a-f]{64}$")
        self.assertTrue(recipe.DAV1D_URL.startswith("https://downloads.videolan.org/"))
        self.assertIn(recipe.DAV1D_VERSION, recipe.DAV1D_URL)
        self.assertIn("--default-library=static", recipe.DAV1D_MESON_ARGS)

    def test_a_dav1d_download_that_does_not_match_the_pin_is_rejected(self):
        from unittest import mock

        response = mock.MagicMock()
        response.__enter__.return_value.read.return_value = b"tampered"
        with mock.patch("urllib.request.urlopen", return_value=response):
            with self.assertRaises(RuntimeError):
                recipe.fetch_dav1d()

    def test_cli_fails_when_a_required_capability_is_missing(self):
        import contextlib
        import io
        import runpy
        from types import SimpleNamespace
        from unittest import mock

        def capture(args, **kwargs):
            output = {
                "-version": "configuration: " + " ".join(recipe.configure_args()),
                "-L": "GNU Lesser General Public License",
                "-encoders": "h264_videotoolbox",
                "-protocols": "https",
            }.get(args[-1], "")  # Deliberately omit the AV1 decoder.
            return SimpleNamespace(stdout=output, returncode=0)

        with mock.patch.object(sys, "argv", ["ffmpeg_lgpl.py", "verify", "fixture"]), \
                mock.patch.object(pathlib.Path, "read_bytes", return_value=LGPL_BINARY), \
                mock.patch("subprocess.run", side_effect=capture), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(SystemExit, "FFmpeg lacks: libdav1d decoder"):
                runpy.run_path(str(pathlib.Path(recipe.__file__)), run_name="__main__")


if __name__ == "__main__":
    unittest.main()
