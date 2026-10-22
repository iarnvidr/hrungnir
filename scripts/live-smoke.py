#!/usr/bin/env python3
"""Real native browser test: enter soccer, inspect Reddit, verify captured posts.

Requires tauri-driver with an isolated XDG_DATA_HOME. No synthetic content or
mocked IPC is used. An inaccessible Reddit page is a failure, never a pass.
"""
import json
import pathlib
import time
import urllib.request
import urllib.error
import http.client
import os
import sqlite3
import signal
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
session = None
data_home=pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert data_home.is_relative_to(root/'var'), 'Use an isolated data directory under the app var/ directory'

def reap_test_app():
    """WebKitWebDriver can orphan a process after losing its session."""
    expected=str(root/'src-tauri/target/debug/offline-reader-desktop').encode()
    marker=('XDG_DATA_HOME='+str(data_home)).encode()
    for process in pathlib.Path('/proc').glob('[0-9]*'):
        try:
            if process.joinpath('cmdline').read_bytes().split(b'\0')[0]==expected and marker in process.joinpath('environ').read_bytes().split(b'\0'):
                os.kill(int(process.name),signal.SIGTERM)
        except (FileNotFoundError,ProcessLookupError,PermissionError):
            pass

def request(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request("http://127.0.0.1:4444" + path, data, {"Content-Type":"application/json"}, method=method)
    try:
        with urllib.request.urlopen(req,timeout=45) as response:
            result=json.load(response)["value"]
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from error
    if isinstance(result,dict) and result.get("error"): raise RuntimeError(result)
    return result

def execute(script,args=None,async_=False):
    return request("POST",f"/session/{session}/execute/{'async' if async_ else 'sync'}",{"script":script,"args":args or []})

def ipc(command,args=None):
    value=execute("const done=arguments[arguments.length-1]; window.__TAURI__.core.invoke(arguments[0],arguments[1]).then(value=>done({value})).catch(error=>done({error:String(error)}))",[command,args or {}],True)
    assert "error" not in value,value
    return value.get("value")

def wait(predicate,timeout=20):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        try:
            result=predicate()
        except (http.client.RemoteDisconnected,ConnectionResetError,urllib.error.URLError):
            result=False
        if result: return result
        time.sleep(.25)
    raise AssertionError("Native condition timed out")

def click(selector):
    element=request("POST",f"/session/{session}/element",{"using":"css selector","value":selector})
    request("POST",f"/session/{session}/element/{element['element-6066-11e4-a52e-4f735466cecf']}/click",{})

def stored_comments(thread_id):
    directory=pathlib.Path(os.environ['XDG_DATA_HOME'])/'xyz.ginnungagaps.offline-reader-exploration'
    with sqlite3.connect(directory/'cache.sqlite') as database:
        row=database.execute('SELECT payload FROM threads WHERE id=?',(thread_id,)).fetchone()
    thread=json.loads(row[0]) if row else None
    return thread if thread and thread['comments'] else None

def center(selector):
    return execute("const r=document.querySelector(arguments[0]).getBoundingClientRect();return [Math.round(r.x+r.width/2),Math.round(r.y+r.height/2)]",[selector])

def native_click(point):
    subprocess.run(['xdotool','mousemove',str(point[0]),str(point[1]),'click','1'],check=True)

def screenshot(name):
    import gi
    gi.require_version('Gdk','3.0')
    from gi.repository import Gdk
    window=Gdk.get_default_root_window()
    target=root/'var/native-smoke'/name
    target.parent.mkdir(parents=True,exist_ok=True)
    Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height()).savev(str(target),'png',[],[])

try:
    start=request("POST","/session",{"capabilities":{"alwaysMatch":{"tauri:options":{"application":str(root/"src-tauri/target/debug/offline-reader-desktop")}}}})
    session=start["sessionId"]
    wait(lambda:execute("return Boolean(window.__TAURI__?.core?.invoke)"))
    wait(lambda:execute("return document.querySelector('#connection-status').textContent.startsWith('Saved ·')"))
    denied=execute("const done=arguments[arguments.length-1];window.__TAURI__.core.invoke('capture_page',{json:'{}'}).then(()=>done(false)).catch(()=>done(true))",async_=True)
    assert denied,'Local controls must not inherit the remote capture capability'
    ipc("prune_cache",{"before":9007199254740991})
    original=request("GET",f"/session/{session}/window")
    element=request("POST",f"/session/{session}/element",{"using":"css selector","value":"#browse-submit"})
    request("POST",f"/session/{session}/element/{element['element-6066-11e4-a52e-4f735466cecf']}/click",{})
    wait(lambda:ipc("browser_status")["mode"]=="live")
    status=wait(lambda:(lambda state:state if state["proxy"]["tunnels"]>0 else None)(ipc("browser_status")),45)
    print("REAL LIVE PROXY:",json.dumps(status),flush=True)
    handles=request("GET",f"/session/{session}/window/handles")
    print("NATIVE WEBVIEWS:",handles,flush=True)
    for handle in handles:
        request("POST",f"/session/{session}/window",{"handle":handle})
        print("PAGE:",execute("return {url:location.href,title:document.title,posts:document.querySelectorAll('shreddit-post,.thing.link').length,text:document.body.innerText.slice(0,600)}"),flush=True)
    request("POST",f"/session/{session}/window",{"handle":original})
    posts=wait(lambda:[thread for thread in ipc("snapshot")["threads"] if thread.get("source")=="browser" and thread["subreddit"].lower()=="soccer"],45)
    print("PASS: actual soccer posts captured",len(posts),flush=True)
    wait(lambda:ipc("browser_status")["loading"] is False,60)
    post=next((post for post in posts if 'discussion' not in post['permalink'].lower()),posts[0])
    saved_point=center('#show-saved')
    click("#show-saved")
    wait(lambda:execute("return document.body.dataset.mode === 'saved' && !document.querySelector('#show-saved').disabled"))
    assert ipc('browser_status')['proxy']['online'] is False
    click(f'[data-thread-id="{post["id"]}"]')
    # WebKitWebDriver loses its inspector session when Tauri closes the child
    # view. Drive the actual button through X11, then inspect persisted content.
    native_click(center('[data-testid="open-live-post"]'))
    captured=wait(lambda:stored_comments(post['id']),60)
    print("PASS: real post comments captured",len(captured["comments"]),flush=True)
    screenshot('live-post.png')
    native_click(saved_point)
    time.sleep(1)
    screenshot('saved-post.png')
    anchor=post['id']
    try: request("DELETE",f"/session/{session}")
    except RuntimeError: pass  # The closed child invalidates WebKit's inspector.
    session=None
    reap_test_app()
    restarted=request("POST","/session",{"capabilities":{"alwaysMatch":{"tauri:options":{"application":str(root/"src-tauri/target/debug/offline-reader-desktop")}}}})
    session=restarted["sessionId"]
    wait(lambda:execute("return Boolean(window.__TAURI__?.core?.invoke)"))
    restored=ipc("snapshot")
    assert restored["position"]==[post["id"],anchor]
    assert next(thread for thread in restored["threads"] if thread["id"]==post["id"])["comments"]
    wait(lambda:ipc("browser_status")["proxy"]["online"] is False)
    print("PASS: actual post/comments and reading position survive process restart in Saved mode",flush=True)
except Exception:
    if session:
        try:
            print("FAILURE UI:",execute("return {status:document.querySelector('#connection-status')?.textContent,feedback:document.querySelector('#browse-feedback')?.textContent}"),flush=True)
            print("FAILURE BROWSER:",ipc("browser_status"),flush=True)
        except Exception as diagnostic_error:
            print("Diagnostic unavailable:",str(diagnostic_error),flush=True)
    raise
finally:
    if not os.environ.get('KEEP_NATIVE_APP'):
        try:
            if session: request("DELETE",f"/session/{session}")
        finally:
            reap_test_app()
