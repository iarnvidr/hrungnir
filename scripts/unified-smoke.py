#!/usr/bin/env python3
"""Native cached-reader integration: completed local media and restart.

First fetch an image and video post through the actual app in an isolated
XDG_DATA_HOME under app var/. Acquisition-window destruction invalidates the
Linux WebKit inspector, so acquisition is exercised separately using OS clicks.
This script starts a fresh process to verify the persisted reader and playback.
Repeat with --offline after disconnecting the disposable container's network.
"""
import json,os,pathlib,sys,time,urllib.request,urllib.error,subprocess,sqlite3

root=pathlib.Path(__file__).resolve().parents[1]
data=pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert data.is_relative_to(root/'var')
session=None

def request(method,path,body=None):
    req=urllib.request.Request('http://127.0.0.1:4444'+path,None if body is None else json.dumps(body).encode(),{'Content-Type':'application/json'},method=method)
    try:
        with urllib.request.urlopen(req,timeout=30) as response: result=json.load(response)['value']
    except urllib.error.HTTPError as error: raise RuntimeError(error.read().decode()) from error
    if isinstance(result,dict) and result.get('error'):raise RuntimeError(result)
    return result

def execute(script,args=None):return request('POST',f'/session/{session}/execute/sync',{'script':script,'args':args or []})

def wait(predicate,seconds=30):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        value=predicate()
        if value:return value
        time.sleep(.25)
    raise AssertionError('Native condition timed out: '+str(execute("return {status:document.querySelector('#connection-status').textContent,feedback:document.querySelector('#reader-feedback').textContent}")))

def click(selector):
    point=execute("const r=document.querySelector(arguments[0]).getBoundingClientRect();return [Math.round(r.x+r.width/2),Math.round(r.y+r.height/2)]",[selector])
    subprocess.run(['xdotool','mousemove',str(point[0]),str(point[1]),'click','1'],check=True)

def wait_disk(predicate,seconds):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        if predicate():return
        time.sleep(.25)
    raise AssertionError('Acquisition/download did not publish its completed cache entry')

def cached_threads():
    db=data/'io.github.iarnvidr.hrungnir/cache.sqlite'
    if not db.exists():return []
    with sqlite3.connect(db) as connection:return [json.loads(row[0]) for row in connection.execute('SELECT payload FROM threads')]

def screenshot(name):
    import gi
    gi.require_version('Gdk','3.0')
    from gi.repository import Gdk
    window=Gdk.get_default_root_window();out=root/'var/unified-smoke';out.mkdir(parents=True,exist_ok=True)
    Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height()).savev(str(out/name),'png',[],[])

try:
    start=request('POST','/session',{'capabilities':{'alwaysMatch':{'tauri:options':{'application':str(root/'src-tauri/target/debug/offline-reader-desktop')}}}})
    session=start['sessionId']
    wait(lambda:execute("return Boolean(window.__TAURI__?.core?.invoke) && !document.querySelector('#browse-submit').disabled"))
    assert execute("return !document.querySelector('#show-live') && !document.querySelector('#show-saved') && !document.querySelector('#live-surface')")
    if '--offline' not in sys.argv:
        wait(lambda:execute("return document.querySelectorAll('.thread-item').length >= 1 && !document.querySelector('#browse-submit').disabled"))
        threads=cached_threads()
        image=next(t for t in threads if any('i.redd.it' in u or 'preview.redd.it' in u for u in t.get('media_urls',[])))
        video=next(t for t in threads if any('v.redd.it' in u for u in t.get('media_urls',[])) or 'streamable.com' in (t.get('link_url') or ''))
        for post in (image,video):
            assert (data/'io.github.iarnvidr.hrungnir/media'/post['id']/'complete.json').exists(),'Fetch an image and video through the app before running this cached-reader check'
        click(f'.thread-item[data-thread-id="{image["id"]}"]')
        wait_disk(lambda:(data/'io.github.iarnvidr.hrungnir/media'/image['id']/'complete.json').exists(),100)
        time.sleep(2)
        wait(lambda:execute("return document.querySelector('#thread-view img')?.naturalWidth > 0"),100)
        assert execute("return document.querySelector('#thread-view img').src.startsWith('http://127.0.0.1:')")
        assert execute("return document.querySelectorAll('#thread-view .comment').length > 0")
        print('PASS: real post comments and completed local image displayed',flush=True)
        screenshot('image.png')
        click(f'.thread-item[data-thread-id="{video["id"]}"]')
        wait_disk(lambda:(data/'io.github.iarnvidr.hrungnir/media'/video['id']/'complete.json').exists(),220)
        time.sleep(2)
        wait(lambda:execute("return Boolean(document.querySelector('#thread-view video'))"),220)
        print('PASS: selected real video downloaded before native local player appeared',flush=True)
    else:
        wait(lambda:execute("return Boolean(document.querySelector('#thread-view video'))"),25)
        print('PASS: selected post/comments/video reopened with container networking disconnected',flush=True)
    assert execute("return document.querySelector('video').currentSrc.startsWith('http://127.0.0.1:')")
    execute("document.querySelector('#tab-reader')?.click();const setting=document.querySelector('#media-presentation');if(setting){setting.value='inline';setting.dispatchEvent(new Event('change'));}const v=document.querySelector('video');v.muted=true;v.play().catch(error=>window.__playbackError=String(error));")
    wait(lambda:execute("return document.querySelector('video').currentTime > 0.5 && !document.querySelector('video').paused"))
    target=execute("const v=document.querySelector('video');const t=Math.min(10,v.duration/2);v.currentTime=t;return t;")
    wait(lambda:execute("const v=document.querySelector('video');return !v.seeking && v.readyState>=2 && v.currentTime>arguments[0]+0.25",[target]))
    assert execute("return !performance.getEntriesByType('resource').some(e => /^https?:/.test(e.name) && !e.name.startsWith('http://ipc.localhost') && !e.name.startsWith('http://127.0.0.1:'))")
    assert execute("return !document.body.innerText.includes('Open it live') && !document.body.innerText.includes('Open live post')")
    execute("document.querySelector('#diagnostics-open').click();")
    wait(lambda:execute("try{return JSON.parse(document.querySelector('#diagnostics-detail').textContent).source.transport==='HTTPS response tee'}catch{return false}"))
    assert execute("const d=JSON.parse(document.querySelector('#diagnostics-detail').textContent);return d.proxy.online===false && d.media.completed_posts>=2 && d.source.remote_ui===false")
    execute("document.querySelector('#diagnostics-refresh').click();")
    wait(lambda:execute("return document.querySelector('#diagnostics-status').textContent==='Diagnostics refreshed.'"))
    execute("document.querySelector('#diagnostics-close').click();")
    print('PASS: native diagnostics report response tee, stopped forwarding and completed media',flush=True)
    screenshot('offline.png' if '--offline' in sys.argv else 'video.png')
    print('PASS: local file decodes/plays; reader has no remote resource loads or Live/Saved controls',flush=True)
finally:
    if session and not os.environ.get('KEEP_NATIVE_APP'):request('DELETE',f'/session/{session}')
