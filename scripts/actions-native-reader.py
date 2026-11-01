#!/usr/bin/env python3
"""Run the native reader probe only from a verified GitHub Actions artifact.

Keep the scratch directory until ``status`` reports a terminal state and the
global runner lock is free. A caller timeout does not mean the native app exited.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import platform
import re
import stat
import subprocess
import sys
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REPO = "iarnvidr/hrungnir"
APP_ID = "io.github.iarnvidr.hrungnir.dev.test"
TARGET = "darwin-aarch64"
WORKFLOW = ".github/workflows/macos.yml"
SCHEMA = 1
MAX_ARCHIVE_BYTES = 2 * 1024 * 1024 * 1024
RUN_LOCK = "native-reader-run.lock"
RUN_STATUS = "native-reader-run-status.json"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
HASH_RE = re.compile(r"^[0-9a-f]{64}$")


class ValidationError(ValueError):
    pass


def _fail(message: str) -> None:
    raise ValidationError(message)


def _status_paths(root: Path) -> tuple[Path, Path]:
    state_dir = root / "var"
    return state_dir / RUN_LOCK, state_dir / RUN_STATUS


def _write_run_status(path: Path, *, phase: str, run_id: int, attempt: int,
                      scratch: Path | None = None, report: Path | None = None,
                      native_log: Path | None = None, child_pid: int | None = None,
                      terminal_state: str | None = None) -> None:
    value = {
        "schema": 1,
        "phase": phase,
        "runner_pid": os.getpid(),
        "child_pid": child_pid,
        "run_id": run_id,
        "run_attempt": attempt,
        "scratch": str(scratch) if scratch else None,
        "report": str(report) if report else None,
        "native_log": str(native_log) if native_log else None,
        "terminal_state": terminal_state,
        "updated_at": datetime.now(timezone.utc).isoformat(),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, separators=(",", ":")) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def _progress(path: Path, state: dict, phase: str, *, terminal_state: str | None = None) -> None:
    state["phase"] = phase
    state["terminal_state"] = terminal_state
    _write_run_status(
        path, phase=phase, run_id=state["run_id"], attempt=state["attempt"],
        scratch=state.get("scratch"), report=state.get("report"),
        native_log=state.get("native_log"), child_pid=state.get("child_pid"),
        terminal_state=terminal_state)
    child = state.get("child_pid")
    child_text = f" child_pid={child}" if isinstance(child, int) else ""
    print(f"Native reader: phase={phase} runner_pid={os.getpid()}{child_text}", flush=True)


def _acquire_run_lock(root: Path) -> tuple[int, Path, Path]:
    lock_path, status_path = _status_paths(root)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(descriptor)
        previous = _read_status(status_path)
        pid = previous.get("child_pid") or previous.get("runner_pid")
        detail = f" (last recorded PID {pid})" if isinstance(pid, int) else ""
        _fail(f"a native reader run is already active{detail}; inspect `status` before retrying")
    return descriptor, lock_path, status_path


def _read_status(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


def runner_status(root: Path = ROOT) -> dict:
    lock_path, status_path = _status_paths(root)
    status = _read_status(status_path)
    if not lock_path.exists():
        return {"state": "idle", "last_run": status or None}
    descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {"state": "running", "run": status or None}
        if not status:
            return {"state": "idle", "last_run": None}
        if status.get("terminal_state") in {"success", "failed"}:
            return {"state": status["terminal_state"], "run": status}
        return {"state": "interrupted_or_stopped", "last_run": status}
    finally:
        os.close(descriptor)


def _json_file(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        _fail(f"cannot read JSON file {path.name}: {error}")
    if not isinstance(value, dict):
        _fail(f"expected a JSON object in {path.name}")
    return value


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _version(root: Path = ROOT) -> str:
    value = _json_file(root / "src-tauri" / "tauri.conf.json").get("version")
    if not isinstance(value, str) or not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?", value):
        _fail("local Tauri config has an invalid version")
    return value


def make_receipt(*, repo: str, run_id: int, run_attempt: int, source_head: str,
                 checkout_sha: str, version: str, archive: Path) -> dict:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        _fail("invalid repository name")
    if run_id < 1 or run_attempt < 1:
        _fail("run id and attempt must be positive integers")
    if not SHA_RE.fullmatch(source_head) or not SHA_RE.fullmatch(checkout_sha):
        _fail("source and checkout revisions must be full 40-character commit IDs")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?", version):
        _fail("invalid application version")
    expected_name = f"Hrungnir-Test-{version}-macos-arm64.zip"
    archive = archive.resolve(strict=True)
    if not archive.is_file() or archive.name != expected_name:
        _fail(f"expected archive named {expected_name}")
    return {
        "schema": SCHEMA,
        "kind": "native-reader-test",
        "repo": repo,
        "run_id": run_id,
        "run_attempt": run_attempt,
        "source_head": source_head,
        "checkout_sha": checkout_sha,
        "version": version,
        "app_identifier": APP_ID,
        "target": TARGET,
        "archive": {"basename": archive.name, "sha256": _sha256(archive)},
    }


def validate_receipt(receipt: dict, *, repo: str, run_id: int, run_attempt: int,
                     version: str) -> dict:
    required = {"schema", "kind", "repo", "run_id", "run_attempt", "source_head",
                "checkout_sha", "version", "app_identifier", "target", "archive"}
    if set(receipt) != required:
        _fail("artifact receipt has unexpected or missing fields")
    if receipt.get("schema") != SCHEMA or receipt.get("kind") != "native-reader-test":
        _fail("artifact receipt has the wrong schema or kind")
    if receipt.get("repo") != repo:
        _fail("receipt repository does not match the requested repository")
    if receipt.get("run_id") != run_id or receipt.get("run_attempt") != run_attempt:
        _fail("receipt is for a different Actions run or attempt")
    if receipt.get("app_identifier") != APP_ID:
        _fail("receipt does not identify the isolated test app")
    if receipt.get("target") != TARGET:
        _fail("receipt target is not Apple Silicon macOS")
    if receipt.get("version") != version:
        _fail("receipt version does not match the current checkout")
    if not SHA_RE.fullmatch(str(receipt.get("source_head", ""))):
        _fail("receipt has an invalid source commit")
    if not SHA_RE.fullmatch(str(receipt.get("checkout_sha", ""))):
        _fail("receipt has an invalid checkout commit")
    archive = receipt.get("archive")
    expected_name = f"Hrungnir-Test-{version}-macos-arm64.zip"
    if (not isinstance(archive, dict) or set(archive) != {"basename", "sha256"}
            or archive.get("basename") != expected_name
            or not HASH_RE.fullmatch(str(archive.get("sha256", "")))):
        _fail("receipt archive name or SHA-256 is invalid")
    return archive


def validate_run(run: dict, *, repo: str, run_id: int, run_attempt: int) -> None:
    if run.get("id") != run_id or run.get("run_attempt") != run_attempt:
        _fail("GitHub returned a different Actions run attempt")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        _fail("the macOS Actions run has not completed successfully")
    workflow_path = str(run.get("path", "")).split("@", 1)[0]
    if workflow_path != WORKFLOW:
        _fail("Actions run was not produced by the macOS workflow")
    repository = run.get("repository")
    if not isinstance(repository, dict) or repository.get("full_name", "").lower() != repo.lower():
        _fail("Actions run belongs to a different repository")
    if run.get("event") not in {"pull_request", "workflow_dispatch"}:
        _fail("Actions run event is not supported for native reader testing")
    if not SHA_RE.fullmatch(str(run.get("head_sha", ""))):
        _fail("Actions run has no valid checkout revision")


def validate_provenance(receipt: dict, run: dict, *, current_head: str,
                        current_pr: dict | None,
                        checkout_commit: dict | None = None) -> None:
    if run.get("event") == "pull_request":
        if not isinstance(current_pr, dict):
            _fail("current checkout has no pull request to match the Actions artifact")
        current_pr_number = current_pr.get("number")
        current_head_oid = current_pr.get("headRefOid")
        current_base_oid = current_pr.get("baseRefOid")
        if (current_head != receipt.get("source_head")
                or current_head_oid != receipt.get("source_head")
                or run.get("head_sha") != receipt.get("source_head")):
            _fail("local HEAD is not the source revision recorded by the current pull request")
        matches = [pr for pr in run.get("pull_requests", [])
                   if pr.get("number") == current_pr_number
                   and pr.get("head", {}).get("sha") == receipt.get("source_head")]
        if not matches:
            _fail("Actions run does not match the current pull request source revision")
        run_base = matches[0].get("base", {}).get("sha")
        if run_base and current_base_oid and run_base != current_base_oid:
            _fail("pull request base changed after the Actions run")
        if (not isinstance(checkout_commit, dict)
                or checkout_commit.get("sha") != receipt.get("checkout_sha")):
            _fail("PR receipt checkout revision could not be verified")
        parent_shas = {parent.get("sha") for parent in checkout_commit.get("parents", [])
                       if isinstance(parent, dict)}
        expected_base = run_base or current_base_oid
        if (not expected_base or receipt.get("source_head") not in parent_shas
                or expected_base not in parent_shas):
            _fail("PR checkout revision is not the recorded source/base merge")
    else:
        if receipt.get("source_head") != receipt.get("checkout_sha"):
            _fail("non-PR receipt source and checkout revisions differ")
        if run.get("head_sha") != receipt.get("checkout_sha"):
            _fail("receipt checkout revision differs from the Actions checkout")
        if current_head != receipt.get("checkout_sha"):
            _fail("local HEAD does not match the non-PR Actions checkout")


def validate_artifact(artifact: dict, *, expected_name: str, run_id: int,
                      checkout_sha: str | None = None) -> None:
    if artifact.get("name") != expected_name or artifact.get("expired") is True:
        _fail("the exact native reader artifact is missing or expired")
    owner = artifact.get("workflow_run") or {}
    if owner.get("id") != run_id:
        _fail("artifact is attached to a different Actions run")
    if checkout_sha is not None and owner.get("head_sha") != checkout_sha:
        _fail("artifact checkout revision differs from the Actions run")
    if not isinstance(artifact.get("id"), int) or artifact["id"] < 1:
        _fail("GitHub artifact has no valid artifact ID")
    if not isinstance(artifact.get("size_in_bytes"), int) or not 1 <= artifact["size_in_bytes"] <= MAX_ARCHIVE_BYTES:
        _fail("GitHub artifact size is missing or exceeds the safety limit")


def extract_downloaded_artifact(artifact_zip: Path, destination: Path, *,
                                allowed_names: set[str]) -> None:
    try:
        with zipfile.ZipFile(artifact_zip) as archive:
            seen = set()
            total_size = 0
            for item in archive.infolist():
                name = item.filename
                mode = item.external_attr >> 16
                file_type = stat.S_IFMT(mode)
                if (not name or "/" in name or "\\" in name or name in {".", ".."}
                        or stat.S_ISLNK(mode)
                        or file_type not in {0, stat.S_IFREG}):
                    _fail("GitHub artifact archive contains an unsafe entry")
                if name in seen or name not in allowed_names:
                    _fail("GitHub artifact archive has duplicate or unexpected files")
                seen.add(name)
                total_size += item.file_size
                if item.file_size > MAX_ARCHIVE_BYTES or total_size > MAX_ARCHIVE_BYTES:
                    _fail("GitHub artifact archive exceeds the safety limit")
            if seen != allowed_names:
                _fail("GitHub artifact archive is missing expected files")
            bad = archive.testzip()
            if bad is not None:
                _fail("GitHub artifact archive has a corrupt member")
            for name in sorted(allowed_names):
                target = destination / name
                with target.open("xb") as output:
                    output.write(archive.read(name))
    except (OSError, zipfile.BadZipFile) as error:
        _fail(f"cannot inspect GitHub artifact archive: {error}")


def verify_archive(archive_path: Path, archive_meta: dict) -> None:
    if not archive_path.is_file() or _sha256(archive_path) != archive_meta.get("sha256"):
        _fail("downloaded archive hash does not match its receipt")


def validate_zip(archive_path: Path, *, expected_app: str) -> list[str]:
    try:
        with zipfile.ZipFile(archive_path) as archive:
            names = []
            seen_names = set()
            total_size = 0
            for item in archive.infolist():
                name = item.filename
                trimmed_name = name[:-1] if name.endswith("/") else name
                raw_parts = trimmed_name.split("/")
                path = PurePosixPath(name)
                mode = item.external_attr >> 16
                if (not name or "\\" in name or path.is_absolute()
                        or not trimmed_name
                        or any(part in {"", ".", ".."} for part in raw_parts)
                        or (raw_parts and re.match(r"^[A-Za-z]:", raw_parts[0]))
                        or item.is_dir() != name.endswith("/")):
                    _fail("archive contains an unsafe path")
                normalized_name = "/".join(raw_parts)
                if normalized_name in seen_names:
                    _fail("archive contains duplicate member paths")
                seen_names.add(normalized_name)
                file_type = stat.S_IFMT(mode)
                if stat.S_ISLNK(mode) or file_type not in {0, stat.S_IFREG, stat.S_IFDIR}:
                    _fail("archive contains an unsupported file type")
                if not raw_parts or raw_parts[0] != expected_app:
                    _fail("archive contains files outside the expected app bundle")
                total_size += item.file_size
                if item.file_size > MAX_ARCHIVE_BYTES or total_size > MAX_ARCHIVE_BYTES:
                    _fail("archive uncompressed size exceeds the safety limit")
                names.append(name.rstrip("/"))
            required = {
                f"{expected_app}/Contents/Info.plist",
                f"{expected_app}/Contents/MacOS/offline-reader-desktop",
            }
            if not required.issubset(set(names)):
                _fail("archive is missing the test app plist or executable")
            bad = archive.testzip()
            if bad is not None:
                _fail("archive has a corrupt member")
            return names
    except (OSError, zipfile.BadZipFile) as error:
        _fail(f"cannot inspect downloaded archive: {error}")


def validate_bundle(app: Path, *, version: str) -> Path:
    info_path = app / "Contents" / "Info.plist"
    executable = app / "Contents" / "MacOS" / "offline-reader-desktop"
    try:
        info = plistlib.loads(info_path.read_bytes())
    except (OSError, plistlib.InvalidFileException) as error:
        _fail(f"cannot read extracted app metadata: {error}")
    if info.get("CFBundleIdentifier") != APP_ID:
        _fail("downloaded app does not use the isolated native test identity")
    if info.get("CFBundleShortVersionString") != version or info.get("CFBundleVersion") != version:
        _fail("downloaded app version does not match the receipt")
    if info.get("CFBundleExecutable") != "offline-reader-desktop":
        _fail("downloaded app executable name is unexpected")
    if info.get("LSUIElement") is not True:
        _fail("downloaded test app is not configured as a background utility")
    if info.get("CFBundleURLTypes"):
        _fail("downloaded test app contains production URL scheme registrations")
    if not executable.is_file() or not os.access(executable, os.X_OK):
        _fail("downloaded test app executable is missing or not executable")
    return executable


def run_command(args: list[str], *, cwd: Path | None = None) -> str:
    try:
        result = subprocess.run(args, cwd=cwd, check=True, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except FileNotFoundError as error:
        _fail(f"required command is unavailable: {Path(error.filename or '').name}")
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip().splitlines()
        _fail(f"command failed ({Path(args[0]).name}, exit {error.returncode})"
              + (f": {detail[-1][:300]}" if detail else ""))
    return result.stdout


def gh_json(args: list[str]) -> dict:
    result = run_command(["gh", "api", *args])
    try:
        value = json.loads(result)
    except json.JSONDecodeError as error:
        _fail(f"GitHub returned invalid JSON: {error}")
    if not isinstance(value, dict):
        _fail("GitHub returned an unexpected response")
    return value


def download_artifact_zip(repo: str, artifact_id: int, destination: Path) -> None:
    args = ["gh", "api", "--header", "Accept: application/vnd.github+json",
            f"repos/{repo}/actions/artifacts/{artifact_id}/zip"]
    try:
        with destination.open("xb") as output:
            result = subprocess.run(args, stdout=output, stderr=subprocess.PIPE, check=False)
    except FileNotFoundError as error:
        _fail(f"required command is unavailable: {Path(error.filename or '').name}")
    except OSError as error:
        _fail(f"cannot write downloaded artifact: {error}")
    if result.returncode != 0:
        destination.unlink(missing_ok=True)
        _fail(f"GitHub artifact download failed (gh api exit {result.returncode})")


def current_pr(root: Path = ROOT) -> dict | None:
    try:
        output = run_command(["gh", "pr", "view", "--json", "number,headRefOid,baseRefOid"], cwd=root)
    except ValidationError:
        return None
    try:
        value = json.loads(output)
    except json.JSONDecodeError:
        _fail("gh pr view returned invalid JSON")
    return value if isinstance(value, dict) else None


def local_head(root: Path) -> str:
    value = run_command(["git", "rev-parse", "HEAD"], cwd=root).strip().lower()
    if not SHA_RE.fullmatch(value):
        _fail("current local checkout does not have a full commit ID")
    return value


def require_clean_checkout(root: Path) -> None:
    status = run_command(["git", "status", "--porcelain", "--untracked-files=no"], cwd=root)
    if status.strip():
        _fail("local checkout has uncommitted tracked changes")


def run_artifact(run_id: int, *, repo: str, root: Path = ROOT,
                 output_dir: Path | None = None) -> Path:
    if run_id < 1 or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        _fail("invalid run id or repository")
    if platform.system() != "Darwin" or platform.machine().lower() not in {"arm64", "aarch64"}:
        _fail("native test artifacts may only be launched on Apple Silicon macOS")
    descriptor, _, status_path = _acquire_run_lock(root)
    state = {"run_id": run_id, "attempt": 0}
    try:
        _progress(status_path, state, "starting")
        result = _run_artifact_locked(run_id, repo=repo, root=root,
                                      output_dir=output_dir, lock_fd=descriptor,
                                      status_path=status_path, state=state)
        _progress(status_path, state, "completed", terminal_state="success")
        return result
    except BaseException:
        _progress(status_path, state, "failed", terminal_state="failed")
        raise
    finally:
        # Do not explicitly unlock: the native child inherits this open file
        # description and keeps the global lock if this runner is terminated.
        os.close(descriptor)


def _run_artifact_locked(run_id: int, *, repo: str, root: Path,
                         output_dir: Path | None, lock_fd: int,
                         status_path: Path, state: dict) -> Path:
    _progress(status_path, state, "checking_repository")
    require_clean_checkout(root)
    try:
        local_repo = json.loads(run_command(["gh", "repo", "view", "--json", "nameWithOwner"], cwd=root))
    except json.JSONDecodeError:
        _fail("gh repo view returned invalid JSON")
    if local_repo.get("nameWithOwner", "").lower() != repo.lower():
        _fail("gh repository context does not match the requested repository")

    _progress(status_path, state, "querying_actions")
    run = gh_json([f"repos/{repo}/actions/runs/{run_id}"])
    attempt = run.get("run_attempt")
    if not isinstance(attempt, int) or attempt < 1:
        _fail("GitHub run has no valid attempt number")
    run = gh_json([f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}"])
    validate_run(run, repo=repo, run_id=run_id, run_attempt=attempt)
    state["attempt"] = attempt
    _progress(status_path, state, "validating_artifact")

    artifact_name = f"hrungnir-native-reader-test-{run_id}-{attempt}"
    listing = gh_json([f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100"])
    artifacts = listing.get("artifacts")
    if not isinstance(artifacts, list):
        _fail("GitHub returned no artifact listing")
    selected = [a for a in artifacts if isinstance(a, dict) and a.get("name") == artifact_name]
    if len(selected) != 1:
        _fail("the exact named native reader artifact is not uniquely available")
    validate_artifact(selected[0], expected_name=artifact_name, run_id=run_id,
                      checkout_sha=run.get("head_sha"))

    run_root = (output_dir.expanduser().resolve() if output_dir is not None
                else root / "var" / "native-reader-artifacts")
    run_root.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix=f"run-{run_id}-{attempt}-", dir=run_root))
    state["scratch"] = scratch
    _progress(status_path, state, "downloading_artifact")
    download = scratch / "download"
    download.mkdir()
    outer_zip = scratch / "github-artifact.zip"
    download_artifact_zip(repo, selected[0]["id"], outer_zip)
    expected_files = {"test-receipt.json", f"Hrungnir-Test-{_version(root)}-macos-arm64.zip"}
    extract_downloaded_artifact(outer_zip, download, allowed_names=expected_files)
    outer_zip.unlink()
    receipt_path = download / "test-receipt.json"
    receipt = _json_file(receipt_path)
    version = _version(root)
    archive_meta = validate_receipt(receipt, repo=repo, run_id=run_id,
                                    run_attempt=attempt, version=version)
    current_head = local_head(root)
    pr = current_pr(root)
    checkout_commit = None
    if run.get("event") == "pull_request":
        checkout_commit = gh_json([f"repos/{repo}/commits/{receipt['checkout_sha']}"])
    validate_provenance(receipt, run, current_head=current_head, current_pr=pr,
                        checkout_commit=checkout_commit)
    archive_path = download / archive_meta["basename"]
    verify_archive(archive_path, archive_meta)

    app_name = "Hrungnir Test.app"
    validate_zip(archive_path, expected_app=app_name)
    extraction = scratch / "extracted"
    extraction.mkdir()
    run_command(["ditto", "-x", "-k", str(archive_path), str(extraction)])
    app = extraction / app_name
    executable = validate_bundle(app, version=version)
    run_command(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)])
    _progress(status_path, state, "artifact_verified")

    # Claim only after every provenance, archive, bundle, and signature check passes.
    run_command([sys.executable, str(root / "scripts" / "native-test-window.py"), "--claim"], cwd=root)
    data_root = scratch / "data"
    data_root.mkdir()
    report = scratch / "native-reader-report.json"
    state["report"] = report
    env = os.environ.copy()
    for name in ("HRUNGNIR_NATIVE_COMMENT_PROBE", "HRUNGNIR_NATIVE_THREAD_PROBE",
                 "HRUNGNIR_NATIVE_COMMENT_THREAD", "HRUNGNIR_NATIVE_PROBE_OFFLINE_VIDEO"):
        env.pop(name, None)
    env.update({
        "HRUNGNIR_DATA_DIR": str(data_root),
        "HRUNGNIR_NATIVE_PROBE": str(report),
        "HRUNGNIR_NATIVE_PROBE_MODE": "reader",
        "HRUNGNIR_NATIVE_UI_REQUIRE_PAGING": "1",
    })
    native_log = scratch / "native.log"
    state["native_log"] = native_log
    print(f"Native reader report: {report}\nNative app log: {native_log}\n"
          f"Runner status: {status_path}", flush=True)
    with native_log.open("wb") as log:
        child = subprocess.Popen([str(executable)], cwd=scratch, env=env,
                                 stdout=log, stderr=subprocess.STDOUT,
                                 pass_fds=(lock_fd,))
        state["child_pid"] = child.pid
        _progress(status_path, state, "native_app_running")
        result_code = child.wait()
    _progress(status_path, state, "native_app_exited")
    if result_code != 0:
        _fail(f"downloaded native test app exited with status {result_code}; see native.log")
    _progress(status_path, state, "validating_native_report")
    run_command([sys.executable, str(root / "scripts" / "native-reader-report.py"), str(report)], cwd=root)
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=__doc__,
        epilog="Do not retry or remove runner scratch while status is running. "
               "A timeout from the calling shell is not proof of completion.")
    sub = parser.add_subparsers(dest="command", required=True)
    receipt = sub.add_parser("receipt", help="write the CI provenance receipt")
    receipt.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", DEFAULT_REPO))
    receipt.add_argument("--run-id", type=int, required=True)
    receipt.add_argument("--run-attempt", type=int, required=True)
    receipt.add_argument("--source-head", required=True)
    receipt.add_argument("--checkout-sha", required=True)
    receipt.add_argument("--version", required=True)
    receipt.add_argument("--archive", type=Path, required=True)
    receipt.add_argument("--output", type=Path, required=True)
    runner = sub.add_parser("run", help="run one verified GitHub native-reader artifact")
    runner.add_argument("run_id", type=int)
    runner.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", DEFAULT_REPO))
    runner.add_argument("--output-dir", type=Path,
                        help="directory for isolated data, downloads, and the native report")
    sub.add_parser("status", help="inspect the active or most recent native reader run")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "receipt":
            value = make_receipt(repo=args.repo, run_id=args.run_id,
                                 run_attempt=args.run_attempt, source_head=args.source_head.lower(),
                                 checkout_sha=args.checkout_sha.lower(), version=args.version,
                                 archive=args.archive)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
            print(args.output)
            return 0
        if args.command == "status":
            value = runner_status()
            print(json.dumps(value, indent=2))
            return 0
        report = run_artifact(args.run_id, repo=args.repo, output_dir=args.output_dir)
        return 0
    except (ValidationError, OSError) as error:
        print(f"Native reader artifact rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
