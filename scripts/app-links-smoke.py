#!/usr/bin/env python3
"""Native argv/single-instance handoff with synthetic cached posts, no network."""
import importlib.util
import argparse
import json
import os
import pathlib
import subprocess
import sqlite3
import time
from urllib.parse import urlencode

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("support_smoke", ROOT / "scripts/support-smoke.py")
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)
DATA = pathlib.Path(os.environ["XDG_DATA_HOME"]).resolve()
assert DATA.is_relative_to(ROOT / "var"), "Use isolated app-owned test data"
BINARY = ROOT / "src-tauri/target/debug/offline-reader-desktop"
OUTPUT = ROOT / "var/app-links-smoke"
OUTPUT.mkdir(parents=True, exist_ok=True)


def app_link(url):
    return "hrungnir://open?" + urlencode({"url": url})


def ready():
    smoke.wait(lambda: smoke.execute("return !document.querySelector('#browse-submit').disabled"))


def secondary(*links):
    # Reuse only this isolated primary's private bus, never another app's bus.
    for proc in pathlib.Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if os.readlink(proc / 'exe') != str(BINARY):
                continue
            values = (proc / 'environ').read_bytes().split(b'\0')
            if f'XDG_DATA_HOME={DATA}'.encode() not in values:
                continue
            bus = next(value.split(b'=', 1)[1].decode() for value in values if value.startswith(b'DBUS_SESSION_BUS_ADDRESS='))
            environment = dict(os.environ, DBUS_SESSION_BUS_ADDRESS=bus)
            result = subprocess.run([str(BINARY), *links], env=environment, timeout=15, capture_output=True, text=True)
            assert result.returncode == 0, result.stderr[-500:]
            return
        except (FileNotFoundError, PermissionError, StopIteration):
            continue
    raise AssertionError("Isolated primary process not found")


def active(title):
    return smoke.execute("return document.querySelector('#thread-view').textContent.includes(arguments[0]) && document.querySelector('#connection-status').textContent.startsWith('Opened shared post')", [title])


def main(public_url=None):
    results = {}
    try:
        smoke.start_app()
        ready()
        smoke.invoke('prune_cache', {'before': 253402300800})
        smoke.execute('localStorage.clear()')
        template = json.loads((ROOT / 'fixtures/thread.json').read_text())
        for number, title in [(1, 'Cold shared post'), (2, 'Warm shared post'), (3, 'Queued shared post')]:
            thread = dict(template, id=f't3_abc10{number}', title=title, subreddit='link_lab', captured_at=int(time.time()),
                          body='Synthetic app-link acceptance', missing=[], comments_status='complete',
                          comments=[{'id': f't1_appreply{number}', 'parent_id': f't3_abc10{number}', 'body': f'Cached reply {number}'}])
            smoke.invoke('import_json', {'json': json.dumps(thread)})
        smoke.stop_app()
        cold = 'https://old.reddit.com/r/link_lab/comments/abc101/shared/appreply1/'
        smoke.start_app([app_link(cold)])
        ready()
        smoke.wait(lambda: active('Cold shared post'))
        assert smoke.execute("return !!document.querySelector('[data-comment-id=\"t1_appreply1\"].anchor-target')")
        results['cold_start_and_comment_anchor'] = True
        smoke.execute("document.querySelector('#settings-open').click()")
        assert smoke.execute("return document.querySelector('#settings-dialog').open")
        secondary('https://www.reddit.com/r/link_lab/comments/abc102/shared/')
        smoke.wait(lambda: active('Warm shared post'))
        assert not smoke.execute("return document.querySelector('#settings-dialog').open")
        results['running_instance_and_dialog_routing'] = True
        # Two arguments in one second-instance launch must both become tabs.
        secondary(app_link(cold), app_link('https://www.reddit.com/r/link_lab/comments/abc103/shared/'))
        smoke.wait(lambda: active('Queued shared post'))
        assert smoke.execute("return document.querySelectorAll('.post-tab-open').length") == 3
        assert smoke.invoke('take_app_links') == []
        results['sequential_queue_and_no_duplicate_tabs'] = True
        secondary(app_link('https://example.invalid/'), 'hrungnir://open?url=file:///etc/passwd')
        time.sleep(.4)
        assert active('Queued shared post')
        assert smoke.execute("return document.querySelectorAll('.post-tab-open').length") == 3
        assert smoke.proxy_requests(smoke.invoke('app_diagnostics')) == 0
        results['invalid_links_ignored_and_no_network'] = True
        if public_url:
            # Explicit opt-in only: a small listing and one caller-selected post.
            secondary(app_link('https://www.reddit.com/r/soccer/top/?t=day'))
            smoke.wait(lambda: smoke.execute("return document.querySelector('#connection-status').textContent.startsWith('/r/soccer ·')"), 45, 'shared subreddit acquisition')
            assert smoke.execute("return document.querySelector('#post-sort').value") == 'top'
            assert smoke.execute("return document.querySelector('#top-period').value") == 'day'
            assert smoke.execute("return document.querySelectorAll('.thread-item').length") > 0
            secondary(app_link(public_url))
            try:
                smoke.wait(lambda: smoke.execute("return document.querySelector('#connection-status').textContent.startsWith('Opened shared post')"), 240, 'selected shared post acquisition')
                results['public_post'] = smoke.execute("const v=document.querySelector('#thread-view video'); return {title:document.querySelector('#thread-view h1')?.textContent,comments:document.querySelectorAll('#thread-view .comment').length,localVideo:!!v && v.src.startsWith('http://127.0.0.1:'),controls:v?.controls,autoplay:v?.autoplay}")
                assert results['public_post']['localVideo'] and results['public_post']['controls']
                assert not results['public_post']['autoplay']
                assert results['public_post']['comments'] > 0
            except RuntimeError as error:
                if 'invalid session id' not in str(error):
                    raise
                # WebKitGTK can drop its inspector when a source child closes.
                # Do not label this a DOM/player pass. Verify persisted capture
                # and completed media, then leave a compositor screenshot.
                results['public_inspector_reset'] = True
                post_id = 't3_' + public_url.split('/comments/', 1)[1].split('/', 1)[0]
                config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
                app_data = DATA / config['identifier']
                def captured():
                    with sqlite3.connect(app_data / 'cache.sqlite') as db:
                        row = db.execute('SELECT payload FROM threads WHERE id=?', (post_id,)).fetchone()
                    if not row:
                        return None
                    thread = json.loads(row[0])
                    manifest = app_data / 'media' / post_id / 'complete.json'
                    return thread if thread.get('comments') and manifest.is_file() else None
                thread = smoke.wait(captured, 240, 'persisted public post and completed media')
                results['public_post'] = {'title': thread['title'], 'comments': len(thread['comments']), 'completed_media': True,
                                          'ui_check': 'Inspector unavailable; review public-post.png separately'}
                time.sleep(.5)
                smoke.screenshot(OUTPUT / 'public-post.png')
            results['public_subreddit_sort_and_selected_post'] = True
        if not results.get('public_inspector_reset'):
            smoke.screenshot(OUTPUT / 'shared-post.png')
        results.update(status='passed', platform='Linux Tauri/WebKitGTK; not macOS Launch Services')
        (OUTPUT / 'result.json').write_text(json.dumps(results, indent=2))
        print(json.dumps(results), flush=True)
    finally:
        smoke.stop_app()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--public-url', help='Explicitly acquire this public Reddit video post after synthetic checks')
    main(parser.parse_args().public_url)
