#!/usr/bin/env python3
"""Native local previews/articles/browser isolation, then a real Reddit search.

Uses only an isolated app-owned var data directory and selected public pages.
"""
import base64
import importlib.util
import json
import os
import pathlib
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('support_smoke', ROOT / 'scripts/support-smoke.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
DATA = pathlib.Path(os.environ['XDG_DATA_HOME']).resolve()
assert DATA.is_relative_to(ROOT / 'var')
APP = DATA / json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())['identifier']
OUT = ROOT / 'var/links-search-smoke'
OUT.mkdir(parents=True, exist_ok=True)


def click(selector):
    s.execute('document.querySelector(arguments[0]).click()', [selector])


def main():
    results = {}
    try:
        s.start_app()
        s.wait(lambda: s.execute('return !document.querySelector("#browse-submit").disabled'))
        thread = json.loads((ROOT / 'fixtures/thread.json').read_text())
        thread.update(id='t3_articlecheck', title='Lamine Yamal public article', subreddit='soccer', source='proxy',
                      captured_at=int(time.time()), comments=[], missing=[], comments_status='empty_confirmed',
                      link_url='https://example.org/', media_urls=[], thumbnail_url='https://b.thumbs.redditmedia.com/test.png')
        directory = APP / 'previews/t3_articlecheck'
        directory.mkdir(parents=True, exist_ok=True)
        (directory / 'thumb.png').write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aYxkAAAAASUVORK5CYII='))
        (directory / 'complete.json').write_text(json.dumps({'sources':[thread['thumbnail_url']], 'files':['thumb.png']}))
        s.invoke('import_json', {'json':json.dumps(thread)})
        s.stop_app()
        s.start_app()
        s.wait(lambda: s.execute('return !document.querySelector("#browse-submit").disabled'))
        s.wait(lambda:s.execute('return !!document.querySelector("[data-thread-id=t3_articlecheck] .link-article-toggle")'))
        s.wait(lambda:s.execute('const i=document.querySelector("[data-preview-for=t3_articlecheck] img");return i?.complete && i.naturalWidth>0'))
        results['local_listing_thumbnail'] = True
        click('[data-thread-id=t3_articlecheck] .link-article-toggle')
        s.wait(lambda:s.execute('return document.querySelector(".link-article-body")?.textContent.includes("documentation")'), 45)
        article = s.invoke('fetch_link_article', {'id':'t3_articlecheck'})
        assert article['cached'] and article['url'].startswith('https://example.org')
        assert not any(item['id']=='t3_articlecheck' for item in s.invoke('library_state')['items'])
        results['inline_article_and_disk_cache_without_library_save'] = True
        s.screenshot(OUT / 'article.png')
        before = s.proxy_requests(s.invoke('app_diagnostics'))
        s.execute('const started=performance.now();window.__cacheSuggestionMs=null;const observer=new MutationObserver(()=>{if(document.querySelector(".cache-search-hit[data-thread-id=t3_articlecheck]")){window.__cacheSuggestionMs=performance.now()-started;observer.disconnect()}});observer.observe(document.querySelector("#cache-search-results"),{childList:true,subtree:true});const i=document.querySelector("#cache-search-input");i.value="Lamine Yamal";i.dispatchEvent(new Event("input",{bubbles:true}))')
        time.sleep(.5)
        assert s.proxy_requests(s.invoke('app_diagnostics')) == before
        s.wait(lambda:s.execute('return !!document.querySelector(".cache-search-hit[data-thread-id=t3_articlecheck]")'),10)
        assert s.proxy_requests(s.invoke('app_diagnostics')) == before
        results['cached_match_while_typing_no_source_request'] = True
        results['cached_suggestion_ms'] = s.execute('return window.__cacheSuggestionMs')
        click('#header-search-submit')
        s.wait(lambda:s.execute('return document.querySelectorAll(".thread-item").length > 1 && document.querySelector(".browser-heading")?.textContent.includes("Lamine Yamal")'),45)
        results['live_reddit_search'] = s.execute('return {heading:document.querySelector(".browser-heading").textContent,rows:document.querySelectorAll(".thread-item").length,communities:[...new Set([...document.querySelectorAll(".thread-subreddit")].map(e=>e.textContent))]}')
        assert s.proxy_requests(s.invoke('app_diagnostics')) > before
        assert s.execute('return document.querySelector("#cache-search-results").hidden')
        results['enter_runs_live_search_and_closes_cache_overlay'] = True
        assert s.execute('return !document.querySelector("#top-period-wrap").hidden && [...document.querySelector("#post-sort").options].filter(o=>!o.disabled).map(o=>o.value).sort().join() === "comments,new,relevance,top"')
        s.execute('const sort=document.querySelector("#post-sort");sort.value="new";sort.dispatchEvent(new Event("change"))')
        s.wait(lambda:s.execute('return !document.querySelector("#header-search-submit").disabled && document.querySelector("#post-sort").value === "new" && document.querySelector("#connection-status").textContent.includes("Reddit results")'),45)
        results['live_search_sort_and_time_controls'] = True
        s.screenshot(OUT / 'reddit-search.png')
        results['status'] = 'passed'
        results['platform'] = 'Linux Tauri/WebKitGTK; macOS runtime pending'
        print(json.dumps(results),flush=True)
    except Exception:
        print(json.dumps(results),flush=True)
        raise
    finally:
        (OUT / 'result.json').write_text(json.dumps(results,indent=2))
        s.stop_app()


if __name__=='__main__':
    main()
