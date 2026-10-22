#!/usr/bin/env python3
"""Assemble a local app bundle around the cross-built ARM executable.

Sign the resulting app with rcodesign before archiving. No signing identity needed
for the local ad-hoc signature; this is not Developer ID signing/notarization.
"""
import pathlib
import plistlib
import json
import shutil
import struct
import sys

root = pathlib.Path(__file__).resolve().parents[1]
config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
binary = root / "src-tauri/target/aarch64-apple-darwin/release/offline-reader-desktop"
magic, cpu = struct.unpack("<II", binary.read_bytes()[:8])
if (magic, cpu) != (0xFEEDFACF, 0x0100000C):
    sys.exit("Expected a 64-bit Apple Silicon Mach-O executable")
product_name = config["productName"]
version = config["version"]
app = root / "artifacts" / f"{product_name}.app"
if app.exists():
    shutil.rmtree(app)
(app / "Contents/MacOS").mkdir(parents=True)
(app / "Contents/Resources").mkdir()
shutil.copy2(binary, app / "Contents/MacOS/offline-reader-desktop")
(app / "Contents/MacOS/offline-reader-desktop").chmod(0o755)
shutil.copy2(root / "src-tauri/icons/icon.icns", app / "Contents/Resources/icon.icns")
if (root / "licenses").exists():
    shutil.copytree(root / "licenses", app / "Contents/Resources/licenses")
tools=root/'var/media-tools'
(app/'Contents/Resources/bin').mkdir()
for name in ('yt-dlp','ffmpeg'):
    if not (tools/name).is_file(): sys.exit('Run scripts/fetch-media-tools.py before packaging')
    shutil.copy2(tools/name,app/'Contents/Resources/bin'/name)
    (app/'Contents/Resources/bin'/name).chmod(0o755)
with (app / "Contents/Info.plist").open("wb") as out:
    plistlib.dump({
        "CFBundleName": product_name,
        "CFBundleDisplayName": product_name,
        "CFBundleIdentifier": config["identifier"],
        "CFBundleExecutable": "offline-reader-desktop",
        "CFBundleIconFile": "icon.icns",
        "CFBundlePackageType": "APPL",
        "CFBundleURLTypes": plistlib.loads((root / "src-tauri/Info.plist").read_bytes())["CFBundleURLTypes"],
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "CFBundleSupportedPlatforms": ["MacOSX"],
        "LSMinimumSystemVersion": config["bundle"]["macOS"]["minimumSystemVersion"],
        "LSRequiresNativeExecution": True,
        "NSHighResolutionCapable": True,
        "NSAppTransportSecurity": plistlib.loads((root / "src-tauri/Info.plist").read_bytes())["NSAppTransportSecurity"],
        "NSPrincipalClass": "NSApplication",
    }, out)
print(app)
