#!/usr/bin/env python3
"""Inject one TLS EOF during one real native listing acquisition.

Requires tauri-driver on :4444, a fresh XDG_DATA_HOME inside this app's var/,
and the native Linux build. The injected connection sends CONNECT only: the
app's actual acquisition worker performs the single public Reddit request.
This verifies Linux native failure-consumer behavior, not macOS recovery.
"""
import json
import os
import pathlib
import signal
import socket
import sqlite3
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "src-tauri/target/debug/offline-reader-desktop"
DATA = pathlib.Path(os.environ["XDG_DATA_HOME"]).resolve()
assert DATA.is_relative_to(ROOT / "var"), "Use isolated app-owned test data"
CONFIG = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
APP_DATA = DATA / CONFIG["identifier"]
assert not APP_DATA.exists(), "Use a fresh test data directory"
DRIVER = os.environ.get("WEBDRIVER_URL", "http://127.0.0.1:4444").rstrip("/")
session = None


def app_pids():
    result = set()
    for path in pathlib.Path("/proc").iterdir():
        if path.name.isdecimal():
            try:
                if (path / "exe").resolve() == BINARY.resolve():
                    result.add(int(path.name))
            except (OSError, RuntimeError):
                pass
    return result


def request(method, path, body=None):
    req = urllib.request.Request(
        DRIVER + path, None if body is None else json.dumps(body).encode(),
        {"Content-Type": "application/json"}, method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error
    value = payload.get("value", payload)
    if isinstance(value, dict) and value.get("error"):
        raise RuntimeError(value)
    return value


def execute(script, args=None):
    return request("POST", f"/session/{session}/execute/sync", {"script": script, "args": args or []})


def wait(predicate, seconds, description):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.025)
    raise AssertionError(f"Timed out waiting for {description}")


def diagnostics():
    execute("""
        window.__reconnectDiagnostics = null;
        window.__TAURI__.core.invoke('app_diagnostics')
          .then(value => { window.__reconnectDiagnostics = {value}; })
          .catch(error => { window.__reconnectDiagnostics = {error:String(error)}; });
        return true;
    """)
    result = wait(lambda: execute("return window.__reconnectDiagnostics"), 10, "diagnostics IPC")
    assert "error" not in result, result
    return result["value"]


def events():
    result = []
    for name in ("hrungnir.previous.jsonl", "hrungnir.jsonl"):
        path = APP_DATA / "logs" / name
        if path.exists():
            for line in path.read_text().splitlines():
                try:
                    result.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # The writer may still be appending its final line.
    return result


before_pids = app_pids()
owned_pids = set()
try:
    session = request("POST", "/session", {"capabilities": {"alwaysMatch": {
        "tauri:options": {"application": str(BINARY)}
    }}})["sessionId"]
    owned_pids = app_pids() - before_pids
    wait(lambda: execute("return Boolean(window.__TAURI__?.core?.invoke) && !document.querySelector('#browse-submit')?.disabled"), 30, "reader startup")
    started = time.monotonic()
    execute("""
        window.__reconnectFetch = {done:false};
        window.__TAURI__.core.invoke('fetch_subreddit', {subreddit:'soccer',sort:'hot',top_period:'day'})
          .then(value => { window.__reconnectFetch = {done:true,posts:value.threads.length}; })
          .catch(error => { window.__reconnectFetch = {done:true,error:String(error)}; });
        return true;
    """)
    report = wait(lambda: (lambda report: report if report["proxy"]["online"] else None)(diagnostics()), 10, "online acquisition proxy")
    port = report["proxy"]["port"]
    assert isinstance(port, int) and 0 < port < 65536
    assert not any(event["event"] == "proxy.body_complete" for event in events()), "Source completed before fault injection"
    # This client deliberately does not send a ClientHello or any HTTP upstream.
    with socket.create_connection(("127.0.0.1", port), timeout=3) as connection:
        connection.sendall(b"CONNECT www.reddit.com:443 HTTP/1.1\r\nHost: www.reddit.com:443\r\n\r\n")
        head = b""
        while not head.endswith(b"\r\n\r\n"):
            byte = connection.recv(1)
            assert byte, "Proxy closed CONNECT before replying"
            head += byte
            assert len(head) < 8192
        assert head.startswith(b"HTTP/1.1 200"), head
    failure = wait(lambda: next((event for event in events() if event["event"] == "proxy.tls_failed" and event["data"].get("retryable")), None), 5, "injected retryable TLS EOF")
    generation = failure["data"]["generation"]
    wait(lambda: next((event for event in events() if event["event"] == "source.connection_retry" and event["data"].get("generation") == generation), None), 5, "native retryable-failure consumer")
    # The original acquisition must survive the injected per-connection failure.
    assert diagnostics()["proxy"]["online"], "Retryable EOF disabled the acquisition proxy"

    def completed_listing():
        observed = events()
        body = next((event for event in observed if event["event"] == "proxy.body_complete" and event["data"].get("generation") == generation and event["sequence"] > failure["sequence"]), None)
        complete = next((event for event in observed if event["event"] == "operation" and event["data"].get("stage") == "source" and event["data"].get("status") == "complete"), None)
        return observed if body and complete else None

    observed = wait(completed_listing, 35, "original source response and successful native cleanup")
    assert any(event["event"] == "proxy.tls_ok" and event["data"].get("generation") == generation for event in observed)
    assert any(event["event"] == "proxy.request" and event["data"].get("generation") == generation for event in observed)
    body = next(event for event in observed if event["event"] == "proxy.body_complete" and event["data"].get("generation") == generation)
    teardown = next(event for event in observed if event["event"] == "operation" and event["data"].get("stage") == "source_teardown")
    assert teardown["sequence"] > body["sequence"], "EOF caused premature source teardown"
    with sqlite3.connect(APP_DATA / "cache.sqlite") as database:
        threads = [json.loads(row[0]) for row in database.execute("SELECT payload FROM threads")]
    assert len(threads) == 25, len(threads)
    assert all(thread.get("source") == "proxy" and thread["subreddit"].lower() == "soccer" for thread in threads)
    result = {"result":"PASS", "platform":"native Linux WebKit", "injected_fault":"CONNECT accepted then TLS EOF", "generation":generation, "posts":len(threads), "elapsed_ms":round((time.monotonic()-started)*1000), "macos_recovery_verified":False}
    output = DATA / "reconnect-result.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result), flush=True)
finally:
    if session:
        try:
            request("DELETE", f"/session/{session}")
        except Exception:
            pass
    for pid in owned_pids & app_pids():
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
