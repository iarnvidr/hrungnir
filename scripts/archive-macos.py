#!/usr/bin/env python3
"""Archive an already signed app, preserving executable permissions."""
import hashlib
import pathlib
import zipfile
import json

app_root = pathlib.Path(__file__).resolve().parents[1]
root = app_root / "artifacts"
config = json.loads((app_root / "src-tauri/tauri.conf.json").read_text())
app = root / f"{config['productName']}.app"
version = config["version"]
archive = root / f"{config['productName']}-{version}-macos-arm64.zip"
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as out:
    for path in sorted(app.rglob("*")):
        if path.is_file():
            out.write(path, path.relative_to(root))
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
checksum = archive.with_suffix(".zip.sha256")
checksum.write_text(f"{digest}  {archive.name}\n")
print(f"{archive}\nSHA256 {digest}")
