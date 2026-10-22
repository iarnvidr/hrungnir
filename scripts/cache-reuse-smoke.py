#!/usr/bin/env python3
"""Native cache reuse acceptance with one local synthetic video; no network."""
import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('support_smoke', ROOT / 'scripts/support-smoke.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
DATA = pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert DATA.is_relative_to(ROOT / 'var')
APP = DATA / json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())['identifier']
OUT = ROOT / 'var/cache-reuse-smoke'
OUT.mkdir(parents=True, exist_ok=True)


def ready():
    s.wait(lambda:s.execute('return !document.querySelector("#browse-submit").disabled'))


try:
    s.start_app()
    ready()
    thread = json.loads((ROOT / 'fixtures/thread.json').read_text())
    thread.update(id='t3_reusecheck', subreddit='soccer', title='Synthetic reusable video', source='proxy',
                  captured_at=int(time.time()), comments=[], missing=[], comments_status='empty_confirmed',
                  link_url='https://v.redd.it/reusecheck', media_urls=['https://v.redd.it/reusecheck/HLSPlaylist.m3u8'])
    directory = APP / 'media/t3_reusecheck'
    directory.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / 'var/compact-fixture.mp4', directory / 'clip.mp4')
    (directory / 'complete.json').write_text(json.dumps({'sources':thread['media_urls'],'files':['clip.mp4']}))
    s.invoke('import_json', {'json':json.dumps(thread)})
    first = s.invoke('fetch_thread', {'id':thread['id']})
    assert first['cache'] == {'comments_hit':True,'media_hit':True}, first['cache']
    before_bytes = hashlib.sha256((directory / 'clip.mp4').read_bytes()).hexdigest()
    before_time = (directory / 'clip.mp4').stat().st_mtime_ns
    before_marker = (directory / 'complete.json').read_bytes()
    thread['media_urls'] = ['https://v.redd.it/reusecheck/DASHPlaylist.mpd?new_signature=example']
    s.invoke('import_json', {'json':json.dumps(thread)})
    assert s.invoke('validate_cached_media', {'id':thread['id']}) is True
    second = s.invoke('fetch_thread', {'id':thread['id']})
    assert second['cache'] == {'comments_hit':True,'media_hit':True}
    assert (directory / 'clip.mp4').stat().st_mtime_ns == before_time
    assert hashlib.sha256((directory / 'clip.mp4').read_bytes()).hexdigest() == before_bytes
    assert (directory / 'complete.json').read_bytes() == before_marker
    s.stop_app()
    s.start_app()
    ready()
    s.wait(lambda:s.execute('return !!document.querySelector(".thread-item[data-thread-id=t3_reusecheck]")'))
    s.execute('document.querySelector(".thread-item[data-thread-id=t3_reusecheck] .thread-title").click()')
    s.wait(lambda:s.execute('return document.querySelector("#connection-status").textContent.includes("Opened from cache")'))
    assert s.proxy_requests(s.invoke('app_diagnostics')) == 0
    assert s.invoke('fetch_thread', {'id':thread['id']})['cache'] == {'comments_hit':True,'media_hit':True}
    result = {'same_clip_hls_dash_reused':True,'file_and_marker_unchanged':True,
              'empty_comments_and_media_reused_after_restart':True,'reader_reports_cache_reuse':True,
              'reddit_requests':0,'platform':'Linux Tauri/WebKitGTK; macOS runtime pending'}
    s.screenshot(OUT / 'cached-post.png')
    (OUT / 'result.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result),flush=True)
finally:
    s.stop_app()
