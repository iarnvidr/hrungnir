#!/usr/bin/env python3
"""Package the verified, signed macOS app for Tauri's signed updater."""
import base64
import datetime
import json
import pathlib
import subprocess
import tarfile
import tempfile


def package(root):
    config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
    version = config['version']
    name = config['productName']
    artifacts = root / 'artifacts'
    app = artifacts / f'{name}.app'
    archive = artifacts / f'{name}-{version}-macos-arm64.app.tar.gz'
    with tarfile.open(archive, 'w:gz', dereference=False) as output:
        output.add(app, arcname=app.name)
    subprocess.run([str(root / 'node_modules/.bin/tauri'), 'signer', 'sign',
                    '--app-version', version, str(archive)], check=True)
    signature = archive.with_suffix(archive.suffix + '.sig').read_text().strip()
    # Independently verify what the CLI signed before publishing the manifest.
    with tempfile.TemporaryDirectory() as temporary:
        public = pathlib.Path(temporary) / 'public.key'
        signed = pathlib.Path(temporary) / 'archive.sig'
        public.write_bytes(base64.b64decode(config['plugins']['updater']['pubkey'], validate=True))
        signed.write_bytes(base64.b64decode(signature, validate=True))
        subprocess.run(['minisign', '-Vm', str(archive), '-p', str(public), '-x', str(signed)], check=True)
    manifest = {
        'version': version,
        'pub_date': datetime.datetime.now(datetime.timezone.utc).isoformat().replace('+00:00', 'Z'),
        'notes': (root / f'docs/RELEASE-{version}.md').read_text(),
        'platforms': {'darwin-aarch64': {
            'signature': signature,
            'url': f'https://github.com/iarnvidr/hrungnir/releases/download/v{version}/{archive.name}',
        }},
    }
    (artifacts / 'latest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Verified updater archive and manifest for {version}')


if __name__ == '__main__':
    package(pathlib.Path(__file__).resolve().parents[1])
