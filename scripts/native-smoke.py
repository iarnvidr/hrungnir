#!/usr/bin/env python3
"""Drive a real Linux Tauri/WebKit window through tauri-driver (no IPC mocks).

Start tauri-driver with DISPLAY and an isolated XDG_DATA_HOME before running.
Uses only Python's standard library. It clears the application's test cache.
"""
import base64
import json
import pathlib
import sys
import time
import urllib.error
import urllib.request

root = pathlib.Path(__file__).resolve().parents[1]
binary = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / "src-tauri/target/debug/offline-reader-desktop"
base = "http://127.0.0.1:4444"
session = None


def request(method, endpoint, payload=None):
    body = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(base + endpoint, body, {"Content-Type": "application/json"}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error
    value = result.get("value")
    if isinstance(value, dict) and value.get("error"):
        raise RuntimeError(value)
    return value


def start():
    global session
    value = request("POST", "/session", {"capabilities": {"alwaysMatch": {
        "tauri:options": {"application": str(binary.resolve())}
    }}})
    session = value["sessionId"]
    wait("return Boolean(window.__TAURI__?.core?.invoke)")
    wait("return document.querySelector('#connection-status')?.textContent.includes('ready')")


def stop():
    global session
    if session:
        request("DELETE", f"/session/{session}")
        session = None


def execute(script, args=None):
    return request("POST", f"/session/{session}/execute/sync", {"script": script, "args": args or []})


def wait(script):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if execute(script):
            return
        time.sleep(0.1)
    raise AssertionError(f"UI condition timed out: {script}")


def ipc(command, args=None):
    value = request("POST", f"/session/{session}/execute/async", {
        "script": "const done=arguments[arguments.length-1]; window.__TAURI__.core.invoke(arguments[0],arguments[1]).then(v=>done({ok:true,value:v})).catch(e=>done({ok:false,error:String(e)}));",
        "args": [command, args or {}],
    })
    assert value["ok"], value
    return value.get("value")


def click(selector):
    element = request("POST", f"/session/{session}/element", {"using": "css selector", "value": selector})
    element_id = element["element-6066-11e4-a52e-4f735466cecf"]
    request("POST", f"/session/{session}/element/{element_id}/click", {})


try:
    start()
    ipc("prune_cache", {"before": 9007199254740991})
    click("#refresh")
    wait("return document.querySelector('#thread-count').textContent === '0'")
    click("#demo-import")
    wait("return document.querySelector('#thread-count').textContent === '1'")
    wait("return document.querySelector('[data-comment-id=\"t1_reply\"]') !== null")
    click('[data-comment-id="t1_reply"] .comment-body')
    deadline = time.monotonic() + 10
    while ipc("snapshot")["position"] != ["t3_fixture", "t1_reply"]:
        assert time.monotonic() < deadline, "comment click did not save position"
        time.sleep(0.1)
    click('[data-comment-id="t1_reply"] .comment-actions button:last-child')
    wait("return document.querySelector('.parent-peek')?.textContent === 'A cached parent comment.'")
    click('[data-comment-id="t1_parent"] > .comment-card .comment-actions button:first-child')
    wait("return document.querySelector('[data-comment-id=\"t1_parent\"] > .children').hidden")
    stop()
    start()
    wait("return document.querySelector('[data-comment-id=\"t1_reply\"]')?.classList.contains('anchor-target')")
    assert ipc("snapshot")["position"] == ["t3_fixture", "t1_reply"]
    print("PASS: real Tauri/WebKit import, nested comments, parent preview, collapse, SQLite position and process-restart restoration")
    # A malformed import must not destroy the current cache.
    result = request("POST", f"/session/{session}/execute/async", {
        "script": "const done=arguments[0]; window.__TAURI__.core.invoke('import_json',{json:'{}'}).then(()=>done(false)).catch(()=>done(true));", "args": []
    })
    assert result is True
    assert len(ipc("snapshot")["threads"]) == 1
    # Untrusted imported text must render as text, not become markup.
    fixture = json.loads((root / "fixtures/thread.json").read_text())
    fixture["title"] = '<img src=x onerror="window.injected=true">'
    ipc("import_json", {"json": json.dumps(fixture)})
    click("#refresh")
    wait("return document.querySelector('#thread-view h1')?.textContent.includes('<img')")
    assert execute("return !window.injected && !document.querySelector('#thread-view img')")
    print("PASS: invalid import preserves data; imported markup renders as plain text")
    # Confirm the destructive action in the test window, preserving real IPC.
    execute("window.confirm = () => true")
    click("#clear-cache")
    wait("return document.querySelector('#thread-count').textContent === '0'")
    assert ipc("snapshot")["position"] is None
    click("#demo-import")
    wait("return document.querySelector('#thread-count').textContent === '1'")
    click('[data-comment-id="t1_reply"] .comment-body')
    deadline = time.monotonic() + 10
    while ipc("snapshot")["position"] != ["t3_fixture", "t1_reply"]:
        assert time.monotonic() < deadline, "position dedup remained stale after clear/reimport"
        time.sleep(0.1)
    print("PASS: clear button removes data and position; reimporting can save the same anchor again")
    output = root / "var/native-smoke/reader.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(base64.b64decode(request("GET", f"/session/{session}/screenshot")))
    print(f"Native WebKit screenshot: {output}")
finally:
    stop()
