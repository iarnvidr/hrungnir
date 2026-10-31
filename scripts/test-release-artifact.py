#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import zipfile

spec = importlib.util.spec_from_file_location('release_artifact', Path(__file__).with_name('release-artifact.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def zip_data(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as archive:
        for name, contents in files.items():
            archive.writestr(name, contents)
    return output.getvalue()


class ReleaseArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'src-tauri').mkdir()
        (self.root / 'src-tauri/tauri.conf.json').write_text('{"version":"1.2.3"}')
        self.commit = 'a' * 40
        self.tree = 'b' * 40
        self.pr_head = 'c' * 40
        self.base = 'd' * 40
        self.workflow_sha = self.commit
        self.name = 'Hrungnir-1.2.3-macos-arm64.zip'
        self.app_archive = zip_data({'Hrungnir.app/Contents/Info.plist': b'fixture app'})
        self.checksum = f'{digest(self.app_archive)}  {self.name}\n'.encode()
        self.files = {self.name: self.app_archive, self.name + '.sha256': self.checksum}
        self.pr = {
            'number': 42, 'merged': True, 'merge_commit_sha': self.commit,
            'base': {'ref': 'main', 'repo': {'full_name': release.REPOSITORY}},
            'head': {'sha': self.pr_head, 'repo': {'full_name': release.REPOSITORY}},
        }
        self.native_run = {
            'id': 123, 'run_attempt': 2, 'path': release.NATIVE_WORKFLOW,
            'repository': {'full_name': release.REPOSITORY},
            'head_repository': {'full_name': release.REPOSITORY},
            'event': 'pull_request', 'head_branch': 'feature', 'head_sha': self.pr_head,
            'conclusion': 'success',
            'pull_requests': [{'number': 42, 'base': {'sha': self.base}}],
        }
        self.ci_run = {
            'id': 124, 'run_attempt': 1, 'path': release.CI_WORKFLOW,
            'repository': {'full_name': release.REPOSITORY},
            'head_repository': {'full_name': release.REPOSITORY},
            'event': 'pull_request', 'head_sha': self.pr_head, 'conclusion': 'success',
            'pull_requests': [{'number': 42, 'base': {'sha': self.base}}],
        }
        self.native_artifact = {
            'id': 456, 'name': f'{release.NATIVE_ARTIFACT_PREFIX}-pr-123-2', 'expired': False,
        }
        self.mapping_run = {
            'id': 789, 'run_attempt': 1, 'path': release.MAPPING_WORKFLOW,
            'repository': {'full_name': release.REPOSITORY}, 'event': 'push',
            'head_branch': 'main', 'head_sha': self.commit, 'conclusion': 'success',
        }
        self.mapping_artifact = {
            'id': 987, 'name': f'{release.MAPPING_ARTIFACT_PREFIX}-{self.commit}-789-1', 'expired': False,
        }
        self.manual_run = {
            'id': 321, 'run_attempt': 1, 'path': release.NATIVE_WORKFLOW,
            'repository': {'full_name': release.REPOSITORY}, 'event': 'workflow_dispatch',
            'head_branch': 'main', 'head_sha': self.commit, 'conclusion': 'success',
        }
        self.manual_artifact = {
            'id': 654, 'name': f'{release.NATIVE_ARTIFACT_PREFIX}-main-321-1', 'expired': False,
        }
        self.native_blob = None
        self.mapping_blob = None
        self.manual_blob = None
        self.include_mapping = True
        self.include_ci = True
        self.policy_drift = False
        self.native_record_tree = self.tree
        self.build_candidate()

    def build_candidate(self, event='pull_request', run=None, artifact=None, tree=None):
        run = run or self.native_run
        artifact = artifact or self.native_artifact
        record = {
            'schema': 2, 'repository': release.REPOSITORY,
            'workflow_path': release.NATIVE_WORKFLOW,
            'run_id': run['id'], 'run_attempt': run['run_attempt'], 'event': event,
            'head': self.pr_head if event == 'pull_request' else self.commit,
            'tree': tree or self.tree, 'workflow_sha': self.workflow_sha,
            'toolchain': {'rustc': 'rustc fixture', 'node': 'v22 fixture', 'macos': '15', 'sdk': 'fixture'},
            'platform': release.PLATFORM, 'version': '1.2.3',
            'files': {name: digest(contents) for name, contents in self.files.items()},
        }
        if event == 'pull_request':
            record.update({'pr_number': 42, 'pr_head_sha': self.pr_head,
                           'tested_base': self.base, 'head_repository': release.REPOSITORY})
        members = {**self.files, 'native-receipt.json': json.dumps(record, sort_keys=True).encode()}
        self.native_blob = zip_data(members)
        artifact['digest'] = 'sha256:' + digest(self.native_blob)

    def git(self, root, *args):
        return self.tree if 'HEAD^{tree}' in args else self.commit

    def api(self, path, binary=False):
        if '/commits/' in path and path.endswith('/pulls'):
            return [{'number': 42, 'base': {'ref': 'main'}}]
        if path.endswith('/pulls/42'):
            return self.pr
        if '/workflows/macos.yml/runs?' in path:
            return {'workflow_runs': [self.native_run]}
        if '/workflows/ci.yml/runs?' in path:
            return {'workflow_runs': [self.ci_run] if self.include_ci else []}
        if '/workflows/accept-native.yml/runs?' in path:
            return {'workflow_runs': [self.mapping_run] if self.include_mapping else []}
        if path.endswith('/actions/runs/123'):
            return self.native_run
        if path.endswith('/actions/runs/321'):
            return self.manual_run
        if path.endswith('/actions/runs/123/artifacts?per_page=100'):
            return {'artifacts': [self.native_artifact]}
        if path.endswith('/actions/runs/321/artifacts?per_page=100'):
            return {'artifacts': [self.manual_artifact]}
        if path.endswith('/actions/runs/789/artifacts?per_page=100'):
            return {'artifacts': [self.mapping_artifact]}
        if path.endswith('/actions/artifacts/456/zip'):
            return self.native_blob
        if path.endswith('/actions/artifacts/654/zip'):
            return self.manual_blob
        if path.endswith('/actions/artifacts/987/zip'):
            return self.mapping_blob
        if '/contents/' in path:
            if self.policy_drift and 'scripts/release-artifact.py' in path and f'ref={self.workflow_sha}' in path:
                return {'sha': 'f' * 40}
            return {'sha': 'e' * 40}
        raise AssertionError(path)

    def promote(self):
        with mock.patch.object(release, 'git', side_effect=self.git):
            return release.promote(self.root, self.commit, self.api)

    def retrieve(self, **kwargs):
        with mock.patch.object(release, 'git', side_effect=self.git):
            return release.retrieve(self.root, 'v1.2.3', api_fn=self.api, **kwargs)

    def install_mapping_bundle(self, mapping):
        self.mapping_blob = zip_data({'accepted-native.json': json.dumps(mapping, sort_keys=True).encode()})
        self.mapping_artifact['digest'] = 'sha256:' + digest(self.mapping_blob)

    def test_main_promoter_maps_exact_merged_same_repo_pr_artifact(self):
        result = self.promote()
        self.assertEqual(result['accepted_commit'], self.commit)
        self.assertEqual(result['accepted_tree'], self.tree)
        self.assertEqual(result['source_run_id'], 123)
        self.assertEqual(result['source_run_attempt'], 2)
        self.assertEqual(result['source_artifact_id'], 456)
        self.assertEqual(result['source_artifact_digest'], self.native_artifact['digest'])
        self.assertEqual(result['ci_run_id'], 124)
        self.assertEqual(json.loads((self.root / 'accepted-native.json').read_text()), result)

    def test_promoter_rejects_fork_even_with_successful_native_run(self):
        self.pr['head']['repo']['full_name'] = 'someone/hrungnir'
        with self.assertRaisesRegex(ValueError, 'fork-produced'):
            self.promote()

    def test_promoter_requires_tree_policy_and_successful_linux_gate(self):
        self.build_candidate(tree='f' * 40)
        with self.assertRaisesRegex(ValueError, 'accepted PR tree'):
            self.promote()
        self.workflow_sha = 'f' * 40
        self.build_candidate()
        self.policy_drift = True
        with self.assertRaisesRegex(ValueError, 'policy differs'):
            self.promote()
        self.policy_drift = False
        self.include_ci = False
        with self.assertRaisesRegex(ValueError, 'successful Linux CI'):
            self.promote()

    def test_promoter_rejects_unsafe_paths_inside_the_native_app_zip(self):
        self.app_archive = zip_data({'Hrungnir.app/../../outside': b'escape'})
        self.checksum = f'{digest(self.app_archive)}  {self.name}\n'.encode()
        self.files = {self.name: self.app_archive, self.name + '.sha256': self.checksum}
        self.build_candidate()
        with self.assertRaisesRegex(ValueError, 'unsafe member'):
            self.promote()

    def test_retrieval_uses_exact_mapped_artifact_id_and_digest_without_rebuilding(self):
        mapping = self.promote()
        self.install_mapping_bundle(mapping)
        result = self.retrieve()
        self.assertEqual(result['source_artifact_id'], 456)
        self.assertEqual(result['acceptance'], 'merged-pr')
        self.assertEqual((self.root / 'artifacts' / self.name).read_bytes(), self.app_archive)
        self.assertEqual(json.loads((self.root / 'artifacts/release-input.json').read_text()), result)

    def test_retrieval_rejects_changed_source_artifact_identity_or_digest(self):
        mapping = self.promote()
        self.install_mapping_bundle(mapping)
        self.native_artifact['digest'] = 'sha256:' + '0' * 64
        with self.assertRaisesRegex(ValueError, 'ID or immutable digest changed'):
            self.retrieve()

    def test_retrieval_rejects_tampered_native_bytes_and_mapping_digest(self):
        mapping = self.promote()
        self.install_mapping_bundle(mapping)
        self.native_blob = self.native_blob + b'tampered'
        with self.assertRaisesRegex(ValueError, 'immutable artifact digest'):
            self.retrieve()
        self.build_candidate()
        self.mapping_blob = zip_data({'accepted-native.json': json.dumps(mapping).encode()})
        self.mapping_artifact['digest'] = 'sha256:' + '0' * 64
        with self.assertRaisesRegex(ValueError, 'immutable artifact digest'):
            self.retrieve()

    def test_manual_trusted_main_fallback_requires_explicit_run_id(self):
        self.include_mapping = False
        self.native_run = self.manual_run
        self.native_artifact = self.manual_artifact
        self.build_candidate(event='workflow_dispatch', run=self.manual_run, artifact=self.manual_artifact)
        self.manual_blob = self.native_blob
        with self.assertRaisesRegex(ValueError, 'no successful main acceptance mapping'):
            self.retrieve()
        result = self.retrieve(trusted_main_run_id=321)
        self.assertEqual(result['acceptance'], 'trusted-main-manual')
        self.assertEqual(result['source_artifact_id'], 654)

    def test_manual_fallback_rejects_push_pr_or_non_main_runs(self):
        self.manual_run['event'] = 'push'
        self.native_run = self.manual_run
        self.native_artifact = self.manual_artifact
        self.build_candidate(event='workflow_dispatch', run=self.manual_run, artifact=self.manual_artifact)
        self.manual_blob = self.native_blob
        with self.assertRaisesRegex(ValueError, 'trusted-main manual native run'):
            self.retrieve(trusted_main_run_id=321)
        self.manual_run['event'] = 'workflow_dispatch'
        self.manual_run['head_branch'] = 'feature'
        with self.assertRaisesRegex(ValueError, 'trusted-main manual native run'):
            self.retrieve(trusted_main_run_id=321)

    def test_tag_version_and_unsafe_mapping_inventory_are_rejected(self):
        with mock.patch.object(release, 'git', side_effect=self.git):
            with self.assertRaisesRegex(ValueError, 'tag does not match'):
                release.retrieve(self.root, 'v9.9.9', api_fn=self.api)
        mapping = self.promote()
        self.install_mapping_bundle(mapping)
        self.mapping_blob = zip_data({'accepted-native.json': json.dumps(mapping).encode(), '../outside': b'bad'})
        self.mapping_artifact['digest'] = 'sha256:' + digest(self.mapping_blob)
        with self.assertRaisesRegex(ValueError, 'unexpected or unsafe'):
            self.retrieve()


if __name__ == '__main__':
    unittest.main()
