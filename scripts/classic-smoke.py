#!/usr/bin/env python3
"""Exercise the classic source in native WebKit; no personal browser data.

Run against tauri-driver on :4444 in a disposable Linux environment with a
fresh XDG_DATA_HOME below this app's var/. Closing acquisition windows can
invalidate WebKit's inspector, so completion is verified from SQLite and logs.
"""
import json
import os
import pathlib
import sqlite3
import subprocess
import sys
import time
import urllib.request

root = pathlib.Path(__file__).resolve().parents[1]
data = pathlib.Path(os.environ["XDG_DATA_HOME"]).resolve()
assert data.is_relative_to(root / "var")
db = data / "xyz.ginnungagaps.offline-reader-exploration/cache.sqlite"
assert not db.exists(), "Use a fresh test data directory"


def request(method, path, body=None):
    req = urllib.request.Request(
        "http://127.0.0.1:4444" + path,
        None if body is None else json.dumps(body).encode(),
        {"Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(req, timeout=30) as response:
        value = json.load(response)["value"]
    if isinstance(value, dict) and value.get("error"):
        raise RuntimeError(value)
    return value


session = request("POST", "/session", {"capabilities": {"alwaysMatch": {
    "tauri:options": {"application": str(root / "src-tauri/target/debug/offline-reader-desktop")}
}}})["sessionId"]


def execute(script):
    return request("POST", f"/session/{session}/execute/sync", {"script": script, "args": []})


try:
    end = time.monotonic() + 20
    while time.monotonic() < end:
        if execute("return !document.querySelector('#browse-submit').disabled"):
            break
        time.sleep(.2)
    # Use an actual OS click to avoid tying completion to a remote inspector
    # connection that WebKit invalidates when the worker is destroyed.
    point = execute("const r=document.querySelector('#browse-submit').getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]")
    started = time.monotonic()
    subprocess.run(["xdotool", "mousemove", str(round(point[0])), str(round(point[1])), "click", "1"], check=True)
    if '--inspect' in sys.argv:
        time.sleep(5)
        handles = request('GET', f'/session/{session}/window/handles')
        print({'windows':len(handles)}, flush=True)
        for handle in handles:
            request('POST', f'/session/{session}/window', {'handle':handle})
            print(execute("return {url:location.href,classic:document.querySelectorAll('.thing.link[data-fullname]').length,modern:document.querySelectorAll('shreddit-post').length,reloading:!!window.__offlineClassicReload,capture:!!window.__offlineReaderCapture,preference:document.cookie.split(';').some(x=>x.trim()==='redesign_optout=true')}"),flush=True)
    end = started + 35
    threads = []
    while time.monotonic() < end:
        if db.exists():
            with sqlite3.connect(db) as connection:
                threads = [json.loads(row[0]) for row in connection.execute("SELECT payload FROM threads")]
        if len(threads) >= 10:
            break
        time.sleep(.2)
    assert len(threads) >= 10, f"Classic listing was not acquired: {len(threads)} posts"
    assert all(t["subreddit"].lower() == "soccer" for t in threads)
    assert all(t.get("source") == "proxy" for t in threads), "Posts must originate from the HTTPS response parser"
    assert all(t["permalink"].startswith("https://www.reddit.com/r/") for t in threads)
    # Let native completion and the local listing render finish before shutdown.
    time.sleep(1)
    print(json.dumps({"result": "PASS", "source": "classic HTML through HTTPS response-capture proxy", "posts": len(threads), "listing_ms": round((time.monotonic()-started)*1000)}), flush=True)
finally:
    try:
        request("DELETE", f"/session/{session}")
    except Exception:
        pass
