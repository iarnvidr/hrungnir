#!/usr/bin/env python3
"""Fetch one selected public post through the native UI in a test data store.

Run classic-smoke.py first. Supply the cached post ID as the first argument.
The acquisition window can invalidate WebKit's inspector, so verify completion
from SQLite and the media manifest rather than pretending the driver survives.
"""
import json, os, pathlib, sqlite3, subprocess, sys, time, urllib.request
root = pathlib.Path(__file__).resolve().parents[1]
data = pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert data.is_relative_to(root / 'var')
post = sys.argv[1]
assert post.startswith('t3_') and post[3:].isalnum()
session = None

def request(method, path, body=None):
    req = urllib.request.Request('http://127.0.0.1:4444'+path, None if body is None else json.dumps(body).encode(), {'Content-Type':'application/json'}, method=method)
    with urllib.request.urlopen(req, timeout=30) as response: value=json.load(response)['value']
    if isinstance(value,dict) and value.get('error'): raise RuntimeError(value)
    return value

def execute(script, args=None):
    return request('POST',f'/session/{session}/execute/sync',{'script':script,'args':args or []})

try:
    session=request('POST','/session',{'capabilities':{'alwaysMatch':{'tauri:options':{'application':str(root/'src-tauri/target/debug/offline-reader-desktop')}}}})['sessionId']
    end=time.monotonic()+25
    selector=f'.thread-item[data-thread-id="{post}"] .thread-open'
    while time.monotonic()<end:
        if execute('return !document.querySelector("#browse-submit").disabled && Boolean(document.querySelector(arguments[0]))',[selector]): break
        time.sleep(.2)
    execute('document.querySelector("#tab-browse").click(); return true;')
    # Navigation is separate from list browsing; opening starts on this action.
    point=execute('const b=document.querySelector(arguments[0]); b.scrollIntoView({block:"center"}); const r=b.getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]',[selector])
    started=time.monotonic()
    subprocess.run(['xdotool','mousemove',str(round(point[0])),str(round(point[1])),'click','1'],check=True)
    end=started+220
    appdata=data/'io.github.iarnvidr.hrungnir'
    manifest=appdata/'media'/post/'complete.json'
    while time.monotonic()<end:
        if manifest.exists(): break
        time.sleep(.25)
    assert manifest.exists(),'Selected post did not publish completed local media'
    with sqlite3.connect(appdata/'cache.sqlite') as connection:
        thread=json.loads(connection.execute('SELECT payload FROM threads WHERE id=?',(post,)).fetchone()[0])
    assert thread['source'] == 'proxy'
    assert thread['comments_status'] in ('partial_proxy','empty_confirmed')
    completed=json.loads(manifest.read_text())
    print(json.dumps({'result':'PASS','post':post,'comments':len(thread['comments']),'media_files':completed['files'],'elapsed_ms':round((time.monotonic()-started)*1000)}),flush=True)
    time.sleep(1)
finally:
    if session:
        try: request('DELETE',f'/session/{session}')
        except Exception: pass
