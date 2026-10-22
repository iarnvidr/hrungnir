#!/usr/bin/env python3
"""Fetch pinned, checksum-verified standalone macOS media helpers."""
import hashlib
import io
import pathlib
import urllib.request
import zipfile

root=pathlib.Path(__file__).resolve().parents[1]
destination=root/'var/media-tools'
destination.mkdir(parents=True,exist_ok=True)
tools=[
 ('yt-dlp','https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos','0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202',None),
 ('ffmpeg','https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip','c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924','ffmpeg'),
]
for name,url,digest,member in tools:
    archive=destination/(name+'.download')
    data=archive.read_bytes() if archive.exists() else urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'offline-reader-build'}),timeout=180).read()
    if hashlib.sha256(data).hexdigest()!=digest: raise SystemExit('Checksum mismatch: '+name)
    archive.write_bytes(data)
    binary=zipfile.ZipFile(io.BytesIO(data)).read(member) if member else data
    (destination/name).write_bytes(binary)
    (destination/name).chmod(0o755)
    print(name+': publisher artifact checksum verified')
