#!/usr/bin/env python3
"""Bind an accepted Hrungnir Apple build to its exact main merge; never rebuild."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import urllib.parse
import urllib.request
from urllib.parse import urlsplit
import zipfile

REPOSITORY = 'iarnvidr/hrungnir'
NATIVE_ARTIFACT_PREFIX = 'hrungnir-macos-arm64'
MAPPING_ARTIFACT_PREFIX = 'hrungnir-accepted-native'
NATIVE_WORKFLOW = '.github/workflows/macos.yml'
MAPPING_WORKFLOW = '.github/workflows/accept-native.yml'
CI_WORKFLOW = '.github/workflows/ci.yml'
HELPER_PATH = 'scripts/release-artifact.py'
PLATFORM = 'darwin-aarch64'
MAX_ARTIFACT_BYTES = 512 * 1024 * 1024
LEGACY_TOOLCHAIN_KEYS = {'rustc', 'node', 'macos', 'sdk'}
TOOLCHAIN_KEYS = {'rustc', 'tauri_cli', 'macos', 'sdk'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.run(['git', '-C', str(root), *args], check=True,
                          capture_output=True, text=True).stdout.strip()


def version(root):
    return json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version']


def valid_toolchain(tools):
    """Accept prior signed receipts while new receipts record the Rust CLI."""
    return isinstance(tools, dict) and set(tools) in (LEGACY_TOOLCHAIN_KEYS, TOOLCHAIN_KEYS)


def request(path, binary=False):
    url = path if path.startswith('https://') else 'https://api.github.com/' + path
    headers = {'Accept': 'application/vnd.github+json',
               'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
               'X-GitHub-Api-Version': '2022-11-28'}
    with urllib.request.build_opener(SafeRedirect()).open(urllib.request.Request(url, headers=headers), timeout=60) as response:
        data = response.read(MAX_ARTIFACT_BYTES + 1)
    if len(data) > MAX_ARTIFACT_BYTES:
        raise ValueError('artifact exceeds bounded download size')
    return data if binary else json.loads(data)


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urlsplit(newurl).scheme != 'https':
            raise ValueError('refusing non-HTTPS artifact or API redirect')
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if redirected is not None and urlsplit(req.full_url).netloc != urlsplit(newurl).netloc:
            redirected.remove_header('Authorization')
        return redirected


def zip_members(data, expected):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        if (len(names) != len(set(names)) or set(names) != set(expected)
                or any(PurePosixPath(name).is_absolute() or '..' in PurePosixPath(name).parts
                       or PurePosixPath(name).name != name for name in names)):
            raise ValueError('unexpected or unsafe artifact inventory')
        if any(info.file_size > MAX_ARTIFACT_BYTES for info in infos):
            raise ValueError('artifact member exceeds bounded size')
        return {name: archive.read(name) for name in names}


def artifact_bytes(artifact, api_fn):
    data = api_fn(f"repos/{REPOSITORY}/actions/artifacts/{artifact['id']}/zip", binary=True)
    if artifact.get('digest') != 'sha256:' + sha(data):
        raise ValueError('GitHub immutable artifact digest mismatch')
    return data


def blob_sha(path, revision, api_fn):
    quoted = urllib.parse.quote(path, safe='/')
    ref = urllib.parse.quote(revision, safe='')
    result = api_fn(f'repos/{REPOSITORY}/contents/{quoted}?ref={ref}')
    digest = result.get('sha')
    if not isinstance(digest, str) or not re.fullmatch(r'[0-9a-f]{40}', digest):
        raise ValueError(f'could not resolve trusted policy file: {path}')
    return digest


def verify_policy_files(workflow_sha, merge_sha, api_fn):
    if not isinstance(workflow_sha, str) or not re.fullmatch(r'[0-9a-f]{40}', workflow_sha):
        raise ValueError('native receipt has no valid workflow revision')
    result = {}
    for path in (NATIVE_WORKFLOW, CI_WORKFLOW, HELPER_PATH):
        merged = blob_sha(path, merge_sha, api_fn)
        produced = blob_sha(path, workflow_sha, api_fn)
        if merged != produced:
            raise ValueError(f'native policy differs from the accepted main source: {path}')
        result[path] = merged
    return result


def receipt(root):
    files = sorted((root / 'artifacts').glob('*.zip'))
    if len(files) != 1:
        raise ValueError('native producer must retain exactly one app archive')
    archive = files[0]
    checksum = archive.with_name(archive.name + '.sha256')
    if checksum.read_text().split()[0] != sha(archive.read_bytes()):
        raise ValueError('archive checksum mismatch')
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    pr = event.get('pull_request') or {}
    pr_head = pr.get('head') or {}
    record = {
        'schema': 2,
        'repository': os.environ['GITHUB_REPOSITORY'],
        'workflow_path': NATIVE_WORKFLOW,
        'run_id': int(os.environ['GITHUB_RUN_ID']),
        'run_attempt': int(os.environ['GITHUB_RUN_ATTEMPT']),
        'event': os.environ['GITHUB_EVENT_NAME'],
        'head': pr_head.get('sha') or os.environ['GITHUB_SHA'],
        'tree': git(root, 'rev-parse', 'HEAD^{tree}'),
        'workflow_sha': os.environ['GITHUB_WORKFLOW_SHA'],
        'toolchain': {name: subprocess.run(command, capture_output=True, text=True, check=True).stdout.strip()
                      for name, command in {'rustc': ['rustc', '--version'],
                                            'tauri_cli': ['cargo-tauri', '--version'],
                                            'macos': ['sw_vers', '-productVersion'],
                                            'sdk': ['xcrun', '--show-sdk-version']}.items()},
        'platform': PLATFORM,
        'version': version(root),
        'files': {p.name: sha(p.read_bytes()) for p in (archive, checksum)},
    }
    if pr:
        record.update({
            'pr_number': pr['number'],
            'pr_head_sha': pr_head.get('sha'),
            'tested_base': (pr.get('base') or {}).get('sha'),
            'head_repository': (pr_head.get('repo') or {}).get('full_name'),
        })
    (root / 'artifacts/native-receipt.json').write_text(json.dumps(record, sort_keys=True) + '\n')


def ci_receipt(root):
    if os.environ.get('GITHUB_EVENT_NAME') != 'pull_request':
        raise ValueError('CI receipt is only produced for pull request runs')
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    pr = event.get('pull_request') or {}
    head = pr.get('head') or {}
    base = pr.get('base') or {}
    record = {
        'schema': 1, 'repository': os.environ['GITHUB_REPOSITORY'],
        'workflow_path': CI_WORKFLOW, 'run_id': int(os.environ['GITHUB_RUN_ID']),
        'run_attempt': int(os.environ['GITHUB_RUN_ATTEMPT']), 'event': 'pull_request',
        'head_sha': head.get('sha'), 'base_sha': base.get('sha'),
        'pr_number': pr.get('number'), 'tested_tree': git(root, 'rev-parse', 'HEAD^{tree}'),
        'workflow_sha': os.environ['GITHUB_WORKFLOW_SHA'],
        'helper_sha': git(root, 'rev-parse', f'HEAD:{HELPER_PATH}'),
    }
    if (not isinstance(record['head_sha'], str) or not isinstance(record['base_sha'], str)
            or not re.fullmatch(r'[0-9a-f]{40}', record['head_sha'])
            or not re.fullmatch(r'[0-9a-f]{40}', record['base_sha'])
            or not isinstance(record['pr_number'], int)):
        raise ValueError('PR CI event has incomplete immutable identity')
    (root / 'artifacts').mkdir(exist_ok=True)
    (root / 'artifacts/ci-receipt.json').write_text(json.dumps(record, sort_keys=True) + '\n')


def validate_run_identity(run, artifact, event, commit):
    if (run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('path') != NATIVE_WORKFLOW or run.get('event') != event
            or run.get('conclusion') != 'success' or run.get('head_sha') != commit):
        raise ValueError('native producer is not the exact successful trusted run')
    if artifact.get('expired') or not isinstance(artifact.get('id'), int):
        raise ValueError('native artifact is missing or expired')
    expected_name = f"{NATIVE_ARTIFACT_PREFIX}-{'pr' if event == 'pull_request' else 'main'}-{run['id']}-{run['run_attempt']}"
    if artifact.get('name') != expected_name:
        raise ValueError('native artifact name does not identify its producing run attempt')
    if not isinstance(artifact.get('digest'), str) or not re.fullmatch(r'sha256:[0-9a-f]{64}', artifact['digest']):
        raise ValueError('native artifact has no immutable digest')


def validate_files(record, members, build_version):
    archive_name = f'Hrungnir-{build_version}-macos-arm64.zip'
    expected_names = {archive_name, archive_name + '.sha256'}
    files = record.get('files')
    if (not isinstance(files, dict) or set(files) != expected_names
            or any(not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value)
                   for value in files.values())):
        raise ValueError('native receipt file inventory is incomplete or unexpected')
    if any(sha(members[name]) != files[name] for name in expected_names):
        raise ValueError('native file checksum mismatch')
    check = members[archive_name + '.sha256'].decode('utf-8', errors='strict').split()
    if not check or check[0] != sha(members[archive_name]):
        raise ValueError('native archive checksum file does not match the app archive')
    validate_app_archive(members[archive_name])
    return files


def validate_app_archive(data):
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            infos = archive.infolist()
            names = [info.filename for info in infos]
            if not infos or len(names) != len(set(names)):
                raise ValueError('native app archive is empty or has duplicate paths')
            total = 0
            for info in infos:
                path = PurePosixPath(info.filename)
                mode = (info.external_attr >> 16) & 0xFFFF
                file_type = stat.S_IFMT(mode)
                if (path.is_absolute() or '..' in path.parts or not path.parts
                        or path.parts[0] != 'Hrungnir.app' or '\\' in info.filename
                        or info.flag_bits & 1 or info.file_size > MAX_ARTIFACT_BYTES
                        or file_type not in (0, stat.S_IFREG, stat.S_IFDIR)):
                    raise ValueError('native app archive contains an unsafe member')
                total += info.file_size
                if total > 1024 * 1024 * 1024:
                    raise ValueError('native app archive exceeds the expanded size limit')
    except zipfile.BadZipFile as error:
        raise ValueError('native app archive is not a valid ZIP') from error


def validate_pr_candidate(data, artifact, run, pr, merge_sha, merge_tree, build_version, api_fn):
    pr_number = pr.get('number')
    pr_head = (pr.get('head') or {}).get('sha')
    if (not pr_number or not pr_head or pr.get('merge_commit_sha') != merge_sha
            or not pr.get('merged') or (pr.get('base') or {}).get('ref') != 'main'
            or (pr.get('base') or {}).get('repo', {}).get('full_name') != REPOSITORY
            or (pr.get('head') or {}).get('repo', {}).get('full_name') != REPOSITORY
            or run.get('head_sha') != pr_head):
        raise ValueError('candidate PR is not the exact merged same-repository main PR')
    validate_run_identity(run, artifact, 'pull_request', pr_head)
    if run.get('head_repository', {}).get('full_name') != REPOSITORY:
        raise ValueError('fork-produced artifacts cannot be promoted')
    members = zip_members(data, {f'Hrungnir-{build_version}-macos-arm64.zip',
                                 f'Hrungnir-{build_version}-macos-arm64.zip.sha256',
                                 'native-receipt.json'})
    record = json.loads(members['native-receipt.json'])
    tested_base = record.get('tested_base')
    if not isinstance(tested_base, str) or not re.fullmatch(r'[0-9a-f]{40}', tested_base):
        raise ValueError('native receipt has no valid tested PR base')
    linked = run.get('pull_requests') or []
    if linked and (len(linked) != 1 or linked[0].get('number') != pr_number
                   or (linked[0].get('base') or {}).get('sha') != tested_base):
        raise ValueError('native run association conflicts with its immutable PR receipt')
    expected = {
        'schema': 2, 'repository': REPOSITORY, 'workflow_path': NATIVE_WORKFLOW,
        'run_id': run['id'], 'run_attempt': run['run_attempt'], 'event': 'pull_request',
        'head': pr_head, 'pr_head_sha': pr_head, 'pr_number': pr_number,
        'head_repository': REPOSITORY, 'tested_base': tested_base,
        'tree': merge_tree, 'version': build_version, 'platform': PLATFORM,
    }
    if any(record.get(key) != value for key, value in expected.items()):
        raise ValueError('native receipt does not match the accepted PR tree, base, or run')
    tools = record.get('toolchain', {})
    if (not valid_toolchain(tools)
            or any(not isinstance(value, str) or not value for value in tools.values())):
        raise ValueError('native toolchain identity is incomplete')
    files = validate_files(record, members, build_version)
    policy_files = verify_policy_files(record.get('workflow_sha'), merge_sha, api_fn)
    return {
        'schema': 1, 'repository': REPOSITORY, 'accepted_commit': merge_sha,
        'accepted_tree': merge_tree, 'version': build_version, 'platform': PLATFORM,
        'pr_number': pr_number, 'pr_head_sha': pr_head, 'source_base_sha': tested_base,
        'source_run_id': run['id'], 'source_run_attempt': run['run_attempt'],
        'source_artifact_id': artifact['id'], 'source_artifact_digest': artifact['digest'],
        'source_artifact_name': artifact['name'], 'workflow_path': NATIVE_WORKFLOW,
        'workflow_sha': record['workflow_sha'], 'policy_files': policy_files,
        'toolchain': tools, 'files': files,
    }


def workflow_runs(workflow, head_sha, event, api_fn):
    found = []
    for page in range(1, 11):
        query = urllib.parse.urlencode({'head_sha': head_sha, 'event': event, 'status': 'success',
                                        'per_page': 100, 'page': page})
        result = api_fn(f'repos/{REPOSITORY}/actions/workflows/{Path(workflow).name}/runs?{query}')
        batch = result.get('workflow_runs', [])
        found.extend(batch)
        if len(batch) < 100:
            break
    return found


def require_ci_gate(pr, tested_base, merge_sha, merge_tree, api_fn):
    head = (pr.get('head') or {}).get('sha')
    number = pr.get('number')
    runs = workflow_runs(CI_WORKFLOW, head, 'pull_request', api_fn)
    candidates = [run for run in runs if run.get('path') == CI_WORKFLOW
                  and run.get('event') == 'pull_request' and run.get('head_sha') == head
                  and run.get('conclusion') == 'success'
                  and run.get('repository', {}).get('full_name') == REPOSITORY
                  and run.get('head_repository', {}).get('full_name') == REPOSITORY]
    accepted = []
    errors = []
    for run in candidates:
        try:
            artifacts = api_fn(f"repos/{REPOSITORY}/actions/runs/{run['id']}/artifacts?per_page=100").get('artifacts', [])
            name = f"hrungnir-ci-receipt-{run['id']}-{run['run_attempt']}"
            found = [item for item in artifacts if item.get('name') == name and not item.get('expired')]
            if len(found) != 1:
                raise ValueError('CI run has no unique unexpired immutable receipt')
            artifact = found[0]
            members = zip_members(artifact_bytes(artifact, api_fn), {'ci-receipt.json'})
            record = json.loads(members['ci-receipt.json'])
            linked = run.get('pull_requests') or []
            expected = {'schema': 1, 'repository': REPOSITORY, 'workflow_path': CI_WORKFLOW,
                        'run_id': run['id'], 'run_attempt': run['run_attempt'], 'event': 'pull_request',
                        'head_sha': head, 'base_sha': tested_base, 'pr_number': number,
                        'tested_tree': merge_tree}
            if (any(record.get(key) != value for key, value in expected.items())
                    or (linked and (len(linked) != 1 or linked[0].get('number') != number
                                    or (linked[0].get('base') or {}).get('sha') != tested_base))):
                raise ValueError('CI receipt does not identify the accepted PR, base, and tested tree')
            policy_files = verify_policy_files(record.get('workflow_sha'), merge_sha, api_fn)
            if record.get('helper_sha') != policy_files[HELPER_PATH]:
                raise ValueError('CI receipt helper differs from the accepted main source')
            accepted.append((run.get('id', 0), run.get('run_attempt', 0), {
                'run_id': run['id'], 'run_attempt': run['run_attempt'],
                'artifact_id': artifact['id'], 'artifact_digest': artifact['digest'],
                'workflow_sha': record['workflow_sha'], 'policy_files': policy_files,
                'tested_tree': merge_tree, 'base_sha': tested_base,
            }))
        except (ValueError, KeyError, TypeError, OSError, zipfile.BadZipFile) as error:
            errors.append(str(error))
    if not accepted:
        reason = errors[-1] if errors else 'no successful trusted-repository Linux CI run for the PR head'
        raise ValueError(f'accepted Linux CI receipt unavailable: {reason}')
    return max(accepted, key=lambda item: (item[0], item[1]))[2]


def verify_ci_receipt(accepted, merge_sha, merge_tree, api_fn):
    reference = accepted.get('ci_receipt')
    if not isinstance(reference, dict):
        raise ValueError('accepted mapping has no exact Linux CI receipt')
    run_id, attempt = reference.get('run_id'), reference.get('run_attempt')
    if not isinstance(run_id, int) or not isinstance(attempt, int):
        raise ValueError('accepted Linux CI run identity is malformed')
    run = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}')
    if (run.get('id') != run_id or run.get('run_attempt') != attempt
            or run.get('path') != CI_WORKFLOW or run.get('event') != 'pull_request'
            or run.get('conclusion') != 'success' or run.get('head_sha') != accepted.get('pr_head_sha')
            or run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('head_repository', {}).get('full_name') != REPOSITORY):
        raise ValueError('Linux CI run is not the successful accepted same-repository PR head')
    linked = run.get('pull_requests') or []
    if linked and (len(linked) != 1 or linked[0].get('number') != accepted.get('pr_number')
                   or (linked[0].get('base') or {}).get('sha') != accepted.get('source_base_sha')):
        raise ValueError('Linux CI run association conflicts with its accepted receipt')
    artifacts = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}/artifacts?per_page=100').get('artifacts', [])
    name = f'hrungnir-ci-receipt-{run_id}-{attempt}'
    found = [item for item in artifacts if item.get('id') == reference.get('artifact_id')
             and item.get('name') == name and item.get('digest') == reference.get('artifact_digest')
             and not item.get('expired')]
    if len(found) != 1:
        raise ValueError('accepted Linux CI artifact ID or immutable digest changed')
    members = zip_members(artifact_bytes(found[0], api_fn), {'ci-receipt.json'})
    record = json.loads(members['ci-receipt.json'])
    expected = {'schema': 1, 'repository': REPOSITORY, 'workflow_path': CI_WORKFLOW,
                'run_id': run_id, 'run_attempt': attempt, 'event': 'pull_request',
                'head_sha': accepted.get('pr_head_sha'), 'base_sha': accepted.get('source_base_sha'),
                'pr_number': accepted.get('pr_number'), 'tested_tree': merge_tree,
                'workflow_sha': reference.get('workflow_sha')}
    if any(record.get(key) != value for key, value in expected.items()):
        raise ValueError('Linux CI receipt no longer matches the accepted PR, base, and tree')
    policy_files = verify_policy_files(record.get('workflow_sha'), merge_sha, api_fn)
    if (record.get('helper_sha') != policy_files[HELPER_PATH]
            or reference.get('policy_files') != policy_files
            or reference.get('tested_tree') != merge_tree
            or reference.get('base_sha') != accepted.get('source_base_sha')):
        raise ValueError('Linux CI receipt policy differs from the accepted main source')


def promote(root, merge_sha, api_fn=request):
    if git(root, 'rev-parse', 'HEAD^{commit}') != merge_sha:
        raise ValueError('promotion checkout does not match the actual main merge commit')
    merge_tree = git(root, 'rev-parse', 'HEAD^{tree}')
    build_version = version(root)
    summaries = api_fn(f'repos/{REPOSITORY}/commits/{merge_sha}/pulls')
    full_prs = [api_fn(f"repos/{REPOSITORY}/pulls/{item['number']}") for item in summaries
                if item.get('number') and (item.get('base') or {}).get('ref') == 'main']
    matches = [pr for pr in full_prs if pr.get('merged') and pr.get('merge_commit_sha') == merge_sha
               and (pr.get('base') or {}).get('ref') == 'main']
    if len(matches) != 1:
        raise ValueError('main commit must correspond to exactly one merged PR')
    pr = matches[0]
    if ((pr.get('head') or {}).get('repo', {}).get('full_name') != REPOSITORY
            or (pr.get('base') or {}).get('repo', {}).get('full_name') != REPOSITORY):
        raise ValueError('fork-produced artifacts cannot be promoted to a release')
    head = pr['head']['sha']
    runs = workflow_runs(NATIVE_WORKFLOW, head, 'pull_request', api_fn)
    candidates = [run for run in runs if run.get('path') == NATIVE_WORKFLOW
                 and run.get('head_sha') == head and run.get('conclusion') == 'success'
                  and run.get('event') == 'pull_request'
                  and run.get('repository', {}).get('full_name') == REPOSITORY
                  and run.get('head_repository', {}).get('full_name') == REPOSITORY]
    accepted = []
    errors = []
    for run in candidates:
        artifacts = api_fn(f"repos/{REPOSITORY}/actions/runs/{run['id']}/artifacts?per_page=100").get('artifacts', [])
        name = f"{NATIVE_ARTIFACT_PREFIX}-pr-{run['id']}-{run['run_attempt']}"
        found = [artifact for artifact in artifacts if artifact.get('name') == name and not artifact.get('expired')]
        if len(found) != 1:
            continue
        artifact = found[0]
        try:
            data = artifact_bytes(artifact, api_fn)
            record = validate_pr_candidate(data, artifact, run, pr, merge_sha, merge_tree, build_version, api_fn)
            ci_receipt_ref = require_ci_gate(pr, record['source_base_sha'], merge_sha,
                                             merge_tree, api_fn)
            accepted.append((run.get('id', 0), run.get('run_attempt', 0), record, ci_receipt_ref))
        except (ValueError, KeyError, TypeError, OSError, zipfile.BadZipFile) as error:
            errors.append(str(error))
    if not accepted:
        reason = errors[-1] if errors else 'no successful native PR artifact for the exact merged head and tree'
        raise ValueError(f'accepted native artifact unavailable: {reason}')
    _, _, record, ci_receipt_ref = max(accepted, key=lambda item: (item[0], item[1]))
    record['ci_receipt'] = ci_receipt_ref
    (root / 'accepted-native.json').write_text(json.dumps(record, sort_keys=True) + '\n')
    return record


def mapping_run(api_fn, merge_sha):
    runs = workflow_runs(MAPPING_WORKFLOW, merge_sha, 'push', api_fn)
    matches = [run for run in runs if run.get('path') == MAPPING_WORKFLOW
               and run.get('head_sha') == merge_sha and run.get('head_branch') == 'main'
               and run.get('repository', {}).get('full_name') == REPOSITORY
               and run.get('conclusion') == 'success']
    if not matches:
        raise ValueError('no successful main acceptance mapping for the requested merge; use the explicit trusted-main fallback if needed')
    return max(matches, key=lambda run: (run.get('id', 0), run.get('run_attempt', 0)))


def read_mapping(api_fn, run, merge_sha, merge_tree, build_version):
    name = f"{MAPPING_ARTIFACT_PREFIX}-{merge_sha}-{run['id']}-{run['run_attempt']}"
    artifacts = api_fn(f"repos/{REPOSITORY}/actions/runs/{run['id']}/artifacts?per_page=100").get('artifacts', [])
    found = [item for item in artifacts if item.get('name') == name and not item.get('expired')]
    if len(found) != 1:
        raise ValueError('main acceptance mapping artifact is missing, expired, or ambiguous')
    members = zip_members(artifact_bytes(found[0], api_fn), {'accepted-native.json'})
    accepted = json.loads(members['accepted-native.json'])
    if (accepted.get('repository') != REPOSITORY or accepted.get('accepted_commit') != merge_sha
            or accepted.get('accepted_tree') != merge_tree or accepted.get('version') != build_version
            or accepted.get('platform') != PLATFORM):
        raise ValueError('accepted mapping does not match the release commit, tree, version, or platform')
    return accepted


def source_artifact(accepted, api_fn):
    run_id = accepted.get('source_run_id')
    attempt = accepted.get('source_run_attempt')
    if not isinstance(run_id, int) or not isinstance(attempt, int):
        raise ValueError('accepted native run identity is malformed')
    run = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}')
    if (run.get('id') != run_id or run.get('run_attempt') != attempt
            or run.get('path') != NATIVE_WORKFLOW or run.get('event') != 'pull_request'
            or run.get('conclusion') != 'success' or run.get('head_sha') != accepted.get('pr_head_sha')
            or run.get('repository', {}).get('full_name') != REPOSITORY
            or run.get('head_repository', {}).get('full_name') != REPOSITORY):
        raise ValueError('source native run is not the accepted successful same-repository PR run')
    artifacts = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}/artifacts?per_page=100').get('artifacts', [])
    found = [item for item in artifacts if item.get('id') == accepted.get('source_artifact_id')
             and item.get('name') == accepted.get('source_artifact_name')
             and item.get('digest') == accepted.get('source_artifact_digest')
             and not item.get('expired')]
    if len(found) != 1:
        raise ValueError('source native artifact ID or immutable digest changed after acceptance')
    pr = api_fn(f"repos/{REPOSITORY}/pulls/{accepted['pr_number']}")
    if (not pr.get('merged') or pr.get('merge_commit_sha') != accepted.get('accepted_commit')
            or (pr.get('base') or {}).get('repo', {}).get('full_name') != REPOSITORY
            or (pr.get('head') or {}).get('repo', {}).get('full_name') != REPOSITORY):
        raise ValueError('accepted PR is no longer the same-repository merge for this release')
    return run, pr, found[0]


def manual_main_candidate(root, run_id, merge_sha, merge_tree, build_version, api_fn):
    if not str(run_id).isdigit():
        raise ValueError('trusted main fallback run ID must be numeric')
    run = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}')
    if (run.get('id') != int(run_id) or run.get('path') != NATIVE_WORKFLOW
            or run.get('event') != 'workflow_dispatch' or run.get('head_branch') != 'main'
            or run.get('head_sha') != merge_sha or run.get('conclusion') != 'success'
            or run.get('repository', {}).get('full_name') != REPOSITORY):
        raise ValueError('fallback must be an exact successful trusted-main manual native run for this tag commit')
    name = f"{NATIVE_ARTIFACT_PREFIX}-main-{run['id']}-{run['run_attempt']}"
    artifacts = api_fn(f'repos/{REPOSITORY}/actions/runs/{run_id}/artifacts?per_page=100').get('artifacts', [])
    found = [item for item in artifacts if item.get('name') == name and not item.get('expired')]
    if len(found) != 1:
        raise ValueError('trusted-main fallback artifact is missing, expired, or ambiguous')
    artifact = found[0]
    data = artifact_bytes(artifact, api_fn)
    validate_run_identity(run, artifact, 'workflow_dispatch', merge_sha)
    expected_files = {f'Hrungnir-{build_version}-macos-arm64.zip',
                      f'Hrungnir-{build_version}-macos-arm64.zip.sha256', 'native-receipt.json'}
    members = zip_members(data, expected_files)
    receipt_record = json.loads(members['native-receipt.json'])
    expected = {'schema': 2, 'repository': REPOSITORY, 'workflow_path': NATIVE_WORKFLOW,
                'run_id': run['id'], 'run_attempt': run['run_attempt'], 'event': 'workflow_dispatch',
                'head': merge_sha, 'tree': merge_tree, 'version': build_version, 'platform': PLATFORM}
    if any(receipt_record.get(key) != value for key, value in expected.items()):
        raise ValueError('manual native receipt does not match the exact trusted main commit')
    if any(key in receipt_record for key in ('pr_number', 'pr_head_sha', 'head_repository')):
        raise ValueError('manual main fallback receipt unexpectedly identifies a PR build')
    tools = receipt_record.get('toolchain', {})
    if not valid_toolchain(tools) or any(not isinstance(v, str) or not v for v in tools.values()):
        raise ValueError('manual native toolchain identity is incomplete')
    files = validate_files(receipt_record, members, build_version)
    policy_files = verify_policy_files(receipt_record.get('workflow_sha'), merge_sha, api_fn)
    return artifact, data, {'schema': 1, 'repository': REPOSITORY,
                            'accepted_commit': merge_sha, 'accepted_tree': merge_tree,
                            'version': build_version, 'platform': PLATFORM,
                            'source_run_id': run['id'], 'source_run_attempt': run['run_attempt'],
                            'source_artifact_id': artifact['id'],
                            'source_artifact_digest': artifact['digest'],
                            'source_artifact_name': artifact['name'],
                            'workflow_path': NATIVE_WORKFLOW,
                            'workflow_sha': receipt_record['workflow_sha'],
                            'policy_files': policy_files, 'toolchain': tools,
                            'files': files, 'acceptance': 'trusted-main-manual'}


def retrieve(root, tag, api_fn=request, trusted_main_run_id=None):
    build_version = version(root)
    if tag != 'v' + build_version:
        raise ValueError('tag does not match source version')
    merge_sha = git(root, 'rev-parse', 'HEAD^{commit}')
    merge_tree = git(root, 'rev-parse', 'HEAD^{tree}')
    if trusted_main_run_id is not None:
        artifact, data, accepted = manual_main_candidate(root, trusted_main_run_id, merge_sha,
                                                         merge_tree, build_version, api_fn)
    else:
        mapping = mapping_run(api_fn, merge_sha)
        accepted = read_mapping(api_fn, mapping, merge_sha, merge_tree, build_version)
        verify_ci_receipt(accepted, merge_sha, merge_tree, api_fn)
        run, pr, artifact = source_artifact(accepted, api_fn)
        data = artifact_bytes(artifact, api_fn)
        verified = validate_pr_candidate(data, artifact, run, pr, merge_sha,
                                         merge_tree, build_version, api_fn)
        for key, value in verified.items():
            if accepted.get(key) != value:
                raise ValueError('source PR artifact no longer matches its accepted main mapping')
        if accepted.get('source_artifact_id') != artifact.get('id') or accepted.get('source_artifact_digest') != artifact.get('digest'):
            raise ValueError('source artifact identity differs from its accepted mapping')
    expected = {f'Hrungnir-{build_version}-macos-arm64.zip',
                f'Hrungnir-{build_version}-macos-arm64.zip.sha256', 'native-receipt.json'}
    members = zip_members(data, expected)
    files = accepted['files']
    payload = {name: members[name] for name in files}
    if any(sha(payload[name]) != digest for name, digest in files.items()):
        raise ValueError('native archive bytes differ from the accepted mapping')
    target = root / 'artifacts'
    target.mkdir(exist_ok=True)
    for name, contents in payload.items():
        (target / name).write_bytes(contents)
    release_input = {key: accepted[key] for key in (
        'repository', 'accepted_commit', 'accepted_tree', 'version', 'platform',
        'source_run_id', 'source_run_attempt', 'source_artifact_id',
        'source_artifact_digest', 'source_artifact_name', 'workflow_path',
        'workflow_sha', 'policy_files', 'toolchain', 'files') if key in accepted}
    release_input['tag'] = tag
    release_input['acceptance'] = accepted.get('acceptance', 'merged-pr')
    (target / 'release-input.json').write_text(json.dumps(release_input, sort_keys=True) + '\n')
    return release_input


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='operation', required=True)
    receipt_parser = sub.add_parser('receipt')
    receipt_parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    ci_receipt_parser = sub.add_parser('ci-receipt')
    ci_receipt_parser.add_argument('--root', type=Path, default=Path.cwd())
    promote_parser = sub.add_parser('promote')
    promote_parser.add_argument('--root', type=Path, default=Path.cwd())
    promote_parser.add_argument('--merge-sha', required=True)
    retrieve_parser = sub.add_parser('retrieve')
    retrieve_parser.add_argument('--root', type=Path, default=Path.cwd())
    retrieve_parser.add_argument('--tag', required=True)
    retrieve_parser.add_argument('--trusted-main-run-id')
    args = parser.parse_args()
    try:
        if args.operation == 'receipt':
            receipt(args.root)
        elif args.operation == 'ci-receipt':
            ci_receipt(args.root)
        elif args.operation == 'promote':
            promote(args.root, args.merge_sha)
        else:
            retrieve(args.root, args.tag, trusted_main_run_id=args.trusted_main_run_id)
    except Exception as exc:
        raise SystemExit(f'native release acceptance blocked: {exc}') from exc
