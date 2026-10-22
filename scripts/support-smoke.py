#!/usr/bin/env python3
"""Exercise native cache hits, appearance persistence and support ZIP export.

Run only after tauri-driver and the native Linux app are ready. The driver must
inherit XDG_DATA_HOME pointing below var/support-smoke/ so the test stays isolated.
"""
import json
import os
import pathlib
import subprocess
import time
import urllib.error
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
SUPPORT_ROOT = (ROOT / "var/support-smoke").resolve()
EXPORT = (SUPPORT_ROOT / "export.zip").resolve()
SAVE_NAME = SUPPORT_ROOT / "export"
WEBDRIVER = os.environ.get("WEBDRIVER_URL", "http://127.0.0.1:4444").rstrip("/")
CONFIG = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
DATA_HOME = pathlib.Path(os.environ.get("XDG_DATA_HOME", "")).resolve()
APP_DATA = DATA_HOME / CONFIG["identifier"]
session = None
LAST_INVOKE_MS = None


def request(method, path, body=None):
    request_body = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        WEBDRIVER + path,
        request_body,
        {"Content-Type": "application/json"},
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            payload = json.load(response)
            return payload.get("value", payload) if isinstance(payload, dict) else payload
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error


def execute(script, args=None):
    return request(
        "POST",
        f"/session/{session}/execute/sync",
        {"script": script, "args": args or []},
    )


def wait(predicate, seconds=30, description="native condition"):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(f"Timed out waiting for {description}")


def invoke(command, args=None, seconds=30):
    global LAST_INVOKE_MS
    execute(
        """
        const started = performance.now();
        window.__supportSmokeCall = { done: false, value: null, error: null, elapsedMs: null };
        window.__TAURI__.core.invoke(arguments[0], arguments[1] || {})
          .then(value => { window.__supportSmokeCall = { done: true, value, error: null, elapsedMs: performance.now() - started }; })
          .catch(error => { window.__supportSmokeCall = { done: true, value: null, error: String(error), elapsedMs: performance.now() - started }; });
        return true;
        """,
        [command, args or {}],
    )
    result = wait(
        lambda: execute("return window.__supportSmokeCall?.done ? window.__supportSmokeCall : null"),
        seconds,
        f"Tauri command {command}",
    )
    if result["error"]:
        raise RuntimeError(f"Tauri command {command} failed: {result['error']}")
    LAST_INVOKE_MS = float(result["elapsedMs"])
    return result["value"]


def start_app(args=None):
    global session
    started = request(
        "POST",
        "/session",
        {
            "capabilities": {
                "alwaysMatch": {
                    "tauri:options": {
                        "application": str(ROOT / "src-tauri/target/debug/offline-reader-desktop"),
                        "args": args or [],
                    }
                }
            }
        },
    )
    if not isinstance(started, dict) or "sessionId" not in started:
        raise RuntimeError(f"tauri-driver did not create a session: {json.dumps(started)}")
    session = started["sessionId"]
    wait(
        lambda: execute(
            "return Boolean(window.__TAURI__?.core?.invoke) && "
            "Boolean(document.querySelector('#theme-select')) && "
            "Boolean(document.querySelector('#support-export'))"
        ),
        60,
        "native reader startup",
    )


def stop_app():
    global session
    if session:
        request("DELETE", f"/session/{session}")
        session = None


def proxy_requests(report):
    stats = report.get("proxy", {}).get("response_stats") or {}
    return int(stats.get("requests", -1))


def run_fetch():
    result = invoke("fetch_thread", {"id": "t3_fixture"}, seconds=15)
    assert result["snapshot"]["threads"]
    assert result["snapshot"]["threads"][0]["id"] == "t3_fixture"
    assert result["media"] == [], "Synthetic fixture should complete without media or network"
    return LAST_INVOKE_MS


def set_appearance(theme, accent):
    actual = execute(
        """
        const theme = document.querySelector('#theme-select');
        const accent = document.querySelector('#accent-select');
        theme.value = arguments[0];
        theme.dispatchEvent(new Event('change', { bubbles: true }));
        accent.value = arguments[1];
        accent.dispatchEvent(new Event('change', { bubbles: true }));
        return {
          theme: document.documentElement.dataset.theme,
          accent: document.documentElement.dataset.accent,
          saved: JSON.parse(localStorage.getItem('offline-reader.preferences.v1'))
        };
        """,
        [theme, accent],
    )
    assert actual["theme"] == theme and actual["accent"] == accent, actual
    assert actual["saved"]["theme"] == theme and actual["saved"]["accent"] == accent, actual


def active_window_title():
    result = subprocess.run(
        ["xdotool", "getactivewindow", "getwindowname"],
        text=True,
        capture_output=True,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def visible_window_titles():
    found = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--name", "."],
        text=True,
        capture_output=True,
        check=False,
    )
    titles = []
    for window in found.stdout.splitlines():
        title = subprocess.run(
            ["xdotool", "getwindowname", window],
            text=True,
            capture_output=True,
            check=False,
        )
        if title.returncode == 0:
            titles.append((window, title.stdout.strip()))
    return titles


def wait_for_save_dialog():
    return wait(
        lambda: next(
            (window for window, title in visible_window_titles() if "Export Hrungnir support ZIP" in title),
            None,
        ),
        20,
        "native support save dialog",
    )


def dialog_key(window, *keys):
    subprocess.run(["xdotool", "windowfocus", "--sync", window], check=True)
    subprocess.run(["xdotool", "key", *keys], check=True)


def canceled_export_status():
    value = execute("return document.querySelector('#diagnostics-status').textContent")
    return value if "canceled" in value.lower() else None


def screenshot(path):
    import gi

    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk

    window = Gdk.get_default_root_window()
    Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height()).savev(
        str(path), "png", [], []
    )


def verify_zip(path):
    with zipfile.ZipFile(path) as archive:
        assert archive.testzip() is None, "Support ZIP failed CRC verification"
        assert set(archive.namelist()) == {
            "report.json",
            "logs.json",
            "timings.json",
            "README.txt",
            "AGENTS.md",
        }, archive.namelist()
        report = json.loads(archive.read("report.json"))
        logs = json.loads(archive.read("logs.json"))
        timings = json.loads(archive.read("timings.json"))

    assert report["schema_version"] == 1 and report["app"] == "Hrungnir", report
    assert report["live"]["source"]["remote_ui"] is False
    assert isinstance(logs, list) and logs, "Expected persistent support log events"
    assert all(
        {"ts_ms", "session", "sequence", "event", "data"} <= set(item) for item in logs
    ), "Support log event schema is incomplete"
    assert len({item["session"] for item in logs}) >= 2, "Restart should create a new log session"
    post_caches = [
        item["data"]
        for item in logs
        if item.get("event") == "post.cache" and item.get("data", {}).get("thread_id") == "t3_fixture"
    ]
    current_calls = post_caches[-3:]
    assert len(current_calls) == 3, post_caches
    assert all(item.get("comments_hit") for item in current_calls), current_calls
    assert not current_calls[0].get("media_hit"), current_calls
    assert all(item.get("media_hit") for item in current_calls[1:]), current_calls
    assert any(
        item.get("stage") == "post" and item.get("status") == "complete"
        for item in report["live"].get("operations", [])
    ), "Live support report should include completed post timings"
    assert any(item.get("stage") == "post" and item.get("samples", 0) >= 1 for item in timings)
    contents = json.dumps(report) + json.dumps(logs) + json.dumps(timings)
    assert "A cached parent comment." not in contents
    assert "Synthetic: a small thread" not in contents
    return {"logs": len(logs), "sessions": len({item["session"] for item in logs}), "timings": timings}


def main():
    assert os.environ.get("XDG_DATA_HOME"), "Set XDG_DATA_HOME before starting tauri-driver"
    assert DATA_HOME.is_relative_to(SUPPORT_ROOT), f"XDG_DATA_HOME must be under {SUPPORT_ROOT}"
    SUPPORT_ROOT.mkdir(parents=True, exist_ok=True)
    assert not EXPORT.exists(), f"Remove the previous smoke artifact first: {EXPORT}"
    assert not list(SUPPORT_ROOT.glob("*.zip")), "Support smoke directory must have no old ZIPs"

    assert CONFIG["productName"] == "Hrungnir"
    assert APP_DATA.is_relative_to(SUPPORT_ROOT)
    try:
        start_app()
        imported = invoke("import_fixture")
        assert any(thread["id"] == "t3_fixture" for thread in imported["threads"])
        # Keep repeat runs deterministic while touching only this smoke fixture's
        # media completion marker and files.
        fixture_media = APP_DATA / "media/t3_fixture"
        if fixture_media.exists():
            import shutil

            shutil.rmtree(fixture_media)
        baseline = proxy_requests(invoke("app_diagnostics"))
        assert baseline >= 0

        first_ms = run_fetch()
        second_ms = run_fetch()
        second_proxy = proxy_requests(invoke("app_diagnostics"))
        assert second_proxy == baseline, f"Cache fetch unexpectedly used proxy: {baseline} -> {second_proxy}"
        assert second_ms < 1000, f"Warm no-media cache hit took {second_ms:.0f} ms"
        print(f"PASS: fixture fetches complete without proxy requests; native invoke cold={first_ms:.1f}ms warm={second_ms:.1f}ms", flush=True)

        for theme in ("system", "classified", "classified_light"):
            for accent in ("blue", "red", "amber", "teal"):
                set_appearance(theme, accent)
        stop_app()

        start_app()
        persisted = execute(
            "return {theme: document.querySelector('#theme-select').value, "
            "accent: document.querySelector('#accent-select').value, "
            "rootTheme: document.documentElement.dataset.theme, "
            "rootAccent: document.documentElement.dataset.accent}"
        )
        assert persisted == {
            "theme": "classified_light",
            "accent": "teal",
            "rootTheme": "classified_light",
            "rootAccent": "teal",
        }, persisted
        restarted_ms = run_fetch()
        restarted_proxy = proxy_requests(invoke("app_diagnostics"))
        assert restarted_proxy == baseline, f"Restart cache hit used proxy: {baseline} -> {restarted_proxy}"
        assert restarted_ms < 1000, f"Restarted cache hit took {restarted_ms:.0f} ms"
        print(f"PASS: theme/accent survived restart; native invoke cache hit={restarted_ms:.1f}ms", flush=True)

        execute("document.querySelector('#diagnostics-open').click()")
        wait(lambda: execute("return document.querySelector('#diagnostics-dialog').open"), description="diagnostics dialog")
        execute("document.querySelector('#support-export').click()")
        wait(lambda: execute("return document.querySelector('#support-export').disabled"), description="support export start")
        dialog = wait_for_save_dialog()
        dialog_key(dialog, "Escape")
        canceled = wait(canceled_export_status, description="canceled support export status")
        assert "canceled" in canceled.lower(), canceled
        assert not EXPORT.exists() and not list(SUPPORT_ROOT.glob("*.zip")), "Canceled dialog created an artifact"
        print("PASS: canceling the native save dialog creates no ZIP", flush=True)

        execute("document.querySelector('#support-export').click()")
        wait(lambda: execute("return document.querySelector('#support-export').disabled"), description="support export start")
        dialog = wait_for_save_dialog()
        subprocess.run(["xdotool", "windowfocus", "--sync", dialog], check=True)
        subprocess.run(["xdotool", "key", "ctrl+l"], check=True)
        # GTK appends the selected filter extension, so type the stem to produce
        # the stable final filename export.zip rather than export.zip.zip.
        subprocess.run(["xdotool", "type", "--clearmodifiers", str(SAVE_NAME)], check=True)
        subprocess.run(["xdotool", "key", "Return"], check=True)
        wait(lambda: EXPORT.is_file(), 20, "support ZIP save")
        wait(
            lambda: "Support ZIP saved" in execute("return document.querySelector('#diagnostics-status').textContent"),
            description="support export completion status",
        )
        saved_status = execute("return document.querySelector('#diagnostics-status').textContent")
        assert str(EXPORT) in saved_status, f"UI did not report the selected ZIP path: {saved_status}"
        assert EXPORT.name == "export.zip"
        zip_result = verify_zip(EXPORT)
        screenshot(SUPPORT_ROOT / "support-export.png")
        print(f"PASS: {EXPORT.name}; five ZIP members; {zip_result['logs']} logs across {zip_result['sessions']} sessions", flush=True)
        print(f"PASS: support evidence screenshot at {SUPPORT_ROOT / 'support-export.png'}", flush=True)
    finally:
        stop_app()


if __name__ == "__main__":
    main()
