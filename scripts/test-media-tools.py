#!/usr/bin/env python3
"""Native helper acceptance: encode, yt-dlp merge and generate a local poster.

Uses generated media and no remote video service. Requires the fetched Mac helpers.
"""
import json
import pathlib
import subprocess
import tempfile

import ffmpeg_lgpl


ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "var/media-tools"
FFMPEG = TOOLS / "ffmpeg"


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True, stdin=subprocess.DEVNULL)


def main():
    ffmpeg_lgpl.verify(FFMPEG)
    missing = ffmpeg_lgpl.verify_capabilities(FFMPEG)
    if missing:
        raise SystemExit("FFmpeg lacks: " + ", ".join(missing))
    with tempfile.TemporaryDirectory(prefix="hrungnir-media-test-") as work:
        work = pathlib.Path(work)
        video, audio = work / "video.mp4", work / "audio.m4a"
        run(FFMPEG, "-nostdin", "-v", "error", "-f", "lavfi", "-i",
            "color=c=blue:s=320x240:r=10", "-t", "1", "-an",
            "-c:v", "h264_videotoolbox", "-pix_fmt", "yuv420p", video)
        run(FFMPEG, "-nostdin", "-v", "error", "-f", "lavfi", "-i",
            "sine=frequency=440:sample_rate=44100", "-t", "1", "-vn",
            "-c:a", "aac", audio)
        info = work / "info.json"
        info.write_text(json.dumps({
            "id": "local-fixture", "title": "Local helper acceptance",
            "extractor": "generic", "extractor_key": "Generic",
            "webpage_url": "https://example.invalid/local-fixture",
            "formats": [
                {"format_id": "v", "url": video.as_uri(), "ext": "mp4",
                 "vcodec": "h264", "acodec": "none", "width": 320, "height": 240},
                {"format_id": "a", "url": audio.as_uri(), "ext": "m4a",
                 "vcodec": "none", "acodec": "aac"},
            ],
        }))
        run(TOOLS / "yt-dlp", "--ignore-config", "--enable-file-urls",
            "--load-info-json", info, "--no-progress", "--no-simulate",
            "-f", "v+a", "--merge-output-format", "mp4",
            "--ffmpeg-location", TOOLS, "-o", work / "merged.%(ext)s")
        merged = work / "merged.mp4"
        # Requiring both streams catches a download that silently skipped merging.
        run(FFMPEG, "-nostdin", "-v", "error", "-i", merged,
            "-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-")
        poster = work / "merged.mp4.poster.jpg"
        run(FFMPEG, "-nostdin", "-loglevel", "error", "-protocol_whitelist",
            "file,pipe", "-i", merged, "-frames:v", "1", "-vf",
            "scale=1280:1280:force_original_aspect_ratio=decrease",
            "-q:v", "3", "-f", "image2", "-y", poster)
        if not 0 < poster.stat().st_size <= 2 * 1024 * 1024:
            raise SystemExit("Poster is empty or exceeds the app's 2 MiB limit")
        run(FFMPEG, "-nostdin", "-v", "error", "-i", poster, "-f", "null", "-")
    print("PASS: H.264/AAC encode, yt-dlp audio/video merge, merged decode, local JPEG poster")


if __name__ == "__main__":
    main()
