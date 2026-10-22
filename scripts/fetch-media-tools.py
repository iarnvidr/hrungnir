#!/usr/bin/env python3
"""Fetch pinned, checksum-verified media helpers for the macOS app.

yt-dlp is downloaded as the publisher's standalone macOS executable. FFmpeg is
built from its official source release (pinned by version and SHA-256) with an
LGPL-only configuration; see scripts/ffmpeg_lgpl.py. The build needs an Apple
Silicon Mac. Elsewhere, set FFMPEG_LGPL_BINARY to a binary built that way; it is
re-verified before use.
"""
import hashlib
import json
import os
import pathlib
import shutil
import sys
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import ffmpeg_lgpl  # noqa: E402

root = pathlib.Path(__file__).resolve().parents[1]
destination = root / "var/media-tools"
destination.mkdir(parents=True, exist_ok=True)

YT_DLP = (
    "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos",
    "0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202",
)
# FFmpeg 9.0.2 official source release; digest of ffmpeg-9.0.2.tar.xz from ffmpeg.org.
FFMPEG = (
    "9.0.2",
    "https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz",
    "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e",
)


def fetch(url, digest, name):
    archive = destination / (name + ".download")
    data = archive.read_bytes() if archive.exists() else urllib.request.urlopen(
        urllib.request.Request(url, headers={"User-Agent": "hrungnir-build"}), timeout=180
    ).read()
    if hashlib.sha256(data).hexdigest() != digest:
        archive.unlink(missing_ok=True)
        raise SystemExit("Checksum mismatch: " + name)
    archive.write_bytes(data)
    return data


def install(name, data):
    (destination / name).write_bytes(data)
    (destination / name).chmod(0o755)


install("yt-dlp", fetch(YT_DLP[0], YT_DLP[1], "yt-dlp"))
print("yt-dlp: publisher artifact checksum verified")

version, url, digest = FFMPEG
override = os.environ.get("FFMPEG_LGPL_BINARY")
if override:
    configuration = ffmpeg_lgpl.verify(pathlib.Path(override))
    shutil.copyfile(override, destination / "ffmpeg")
    (destination / "ffmpeg").chmod(0o755)
else:
    stamp = destination / "ffmpeg.source-sha256"
    built = destination / "ffmpeg"
    try:
        if not (built.is_file() and stamp.exists() and stamp.read_text().strip() == digest):
            raise ffmpeg_lgpl.LicenseError("no matching build yet")
        configuration = ffmpeg_lgpl.verify(built)
        print("ffmpeg: reusing the verified build for this source pin")
    except ffmpeg_lgpl.LicenseError:
        tarball = fetch(url, digest, "ffmpeg-source")
        try:
            install("ffmpeg", ffmpeg_lgpl.build(tarball))
        except ffmpeg_lgpl.BuildUnavailable as error:
            raise SystemExit(str(error))
        configuration = ffmpeg_lgpl.verify(built)
        stamp.write_text(digest + "\n")
(destination / "ffmpeg.configuration.txt").write_text(configuration + "\n")
print(f"ffmpeg {version}: LGPL-only build verified")
print("configuration: " + configuration)
