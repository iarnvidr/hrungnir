#!/usr/bin/env python3
"""Archive an already signed app, preserving executable permissions."""
import hashlib
import argparse
import pathlib
import zipfile
import json

app_root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--test", action="store_true", help="Archive the isolated Actions test bundle")
args = parser.parse_args()
root = app_root / "artifacts" / "native-test" if args.test else app_root / "artifacts"
config = json.loads((app_root / "src-tauri/tauri.conf.json").read_text())
product = "Hrungnir Test" if args.test else config["productName"]
app = root / f"{product}.app"
version = config["version"]
archive_name = "Hrungnir-Test" if args.test else product
archive = root / f"{archive_name}-{version}-macos-arm64.zip"
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as out:
    for path in sorted(app.rglob("*")):
        if path.is_file():
            out.write(path, path.relative_to(root))
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
checksum = archive.with_suffix(".zip.sha256")
checksum.write_text(f"{digest}  {archive.name}\n")
print(f"{archive}\nSHA256 {digest}")
