#!/usr/bin/env python3
"""Exercise real updater signing and independently reject tampered archives."""
import base64
import importlib.util
import json
import os
import pathlib
import subprocess
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('package_updater', ROOT / 'scripts/package-updater.py')
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)


class UpdaterPackageTest(unittest.TestCase):
    def test_signed_archive_and_tamper_rejection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            signer = updater.tauri_cli()
            key = root / 'test.key'
            subprocess.run([signer, 'signer', 'generate', '--ci', '-w', str(key)],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            public = key.with_suffix('.key.pub').read_text().strip()
            (root / 'src-tauri').mkdir()
            (root / 'src-tauri/tauri.conf.json').write_text(json.dumps({
                'productName': 'Hrungnir', 'version': '1.2.3',
                'plugins': {'updater': {'pubkey': public}},
            }))
            (root / 'docs').mkdir()
            (root / 'docs/RELEASE-1.2.3.md').write_text('Synthetic release notes')
            binary = root / 'artifacts/Hrungnir.app/Contents/MacOS/reader'
            binary.parent.mkdir(parents=True)
            binary.write_text('#!/bin/sh\nexit 0\n')
            binary.chmod(0o755)
            binary.with_name('alias').symlink_to('reader')
            previous = os.environ.get('TAURI_SIGNING_PRIVATE_KEY')
            os.environ['TAURI_SIGNING_PRIVATE_KEY'] = key.read_text().strip()
            os.environ['TAURI_SIGNING_PRIVATE_KEY_PASSWORD'] = ''
            try:
                updater.package(root)
            finally:
                if previous is None:
                    os.environ.pop('TAURI_SIGNING_PRIVATE_KEY', None)
                else:
                    os.environ['TAURI_SIGNING_PRIVATE_KEY'] = previous
            manifest = json.loads((root / 'artifacts/latest.json').read_text())
            self.assertEqual(manifest['version'], '1.2.3')
            platform = manifest['platforms']['darwin-aarch64']
            archive = root / 'artifacts/Hrungnir-1.2.3-macos-arm64.app.tar.gz'
            self.assertTrue(platform['url'].endswith('/v1.2.3/' + archive.name))
            with tarfile.open(archive) as contents:
                self.assertEqual(contents.getmember('Hrungnir.app/Contents/MacOS/reader').mode & 0o777, 0o755)
                self.assertTrue(contents.getmember('Hrungnir.app/Contents/MacOS/alias').issym())
            signature = root / 'signature'
            signature.write_bytes(base64.b64decode(platform['signature']))
            verification_key = root / 'public'
            verification_key.write_bytes(base64.b64decode(public))
            with archive.open('ab') as output:
                output.write(b'tampered')
            verified = subprocess.run(['minisign', '-Vm', str(archive), '-p', str(verification_key), '-x', str(signature)],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.assertNotEqual(verified.returncode, 0, 'tampered update passed verification')


if __name__ == '__main__':
    unittest.main()
