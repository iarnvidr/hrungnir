#!/usr/bin/env python3
"""Run reader search/list ownership regressions in a Playwright Chromium native-command stub."""
from __future__ import annotations

import http.server
import json
import os
import pathlib
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
if "PWCLI" not in os.environ:
    raise SystemExit("Set PWCLI to the path of a Playwright CLI wrapper script (playwright_cli.sh).")
PWCLI = pathlib.Path(os.environ["PWCLI"])
SESSION = f"hrungnir-search-ui-{uuid.uuid4().hex[:8]}"
THREAD = {
    "id": "t3_stub", "subreddit": "soccer", "title": "Stub tab", "author": "tester",
    "selftext": "", "url": "https://www.reddit.com/r/soccer/comments/stub/",
    "permalink": "/r/soccer/comments/stub/", "created_utc": 1, "score": 1,
    "num_comments": 0, "comments": [], "comments_status": "empty_confirmed",
    "media_urls": [], "thumbnail_url": None, "link_url": None, "source": "proxy",
}

STUB_JS = r"""(() => {
  const thread = __THREAD__;
  const snapshot = (threads = [], media_hints = {}) => ({threads, diagnostics:{platform:{os:'linux'},source:{},recent_operations:[],active_operations:[]},media_hints,link_previews:{},position:null});
  const calls = {search_cache:[], search_reddit:[], fetch_subreddit:[], fetch_thread:[], previewFetchCount:0};
  const state = {validateCachedMedia:true};
  const immediate = {
    snapshot: async () => snapshot([thread], {t3_stub:{kind:'video',count:1}}),
    library_state: async () => ({groups:[],items:[]}),
    app_diagnostics: async () => ({report:{platform:{os:'linux'},source:{},recent_operations:[],active_operations:[]}}),
    fetch_link_previews: async () => { calls.previewFetchCount++; return {}; },
    get_cache_settings: async () => ({post_cache_hours:48}),
    open_cached_thread: async () => ({snapshot:snapshot([thread], {t3_stub:{kind:'video',count:1}}),media:[]}),
    validate_cached_media: async () => state.validateCachedMedia,
    save_position: async () => null,
    take_app_links: async () => [],
  };
  window.__stub = {
    calls,
    state,
    resolve(name, index, value) { calls[name][index].resolve(value); },
    reject(name, index, reason) { calls[name][index].reject(new Error(reason)); },
    snapshot,
  };
  window.__TAURI__ = {
    core: {invoke(name,args) {
      if (calls[name]) return new Promise((resolve,reject) => calls[name].push({args,resolve,reject}));
      if (immediate[name]) return immediate[name](args);
      return Promise.resolve(null);
    }},
    event: {listen: async () => () => {}},
  };
  window.__searchUiSmoke = async () => {
    const assert = (condition, message) => { if (!condition) throw new Error(message); };
    const pause = () => new Promise(resolve => setTimeout(resolve, 0));
    const wait = async (predicate, message) => {
      const deadline = Date.now() + 4000;
      while (!predicate()) { if (Date.now() > deadline) throw new Error(`Timed out: ${message}`); await pause(); }
    };
    const submitSearch = async (query) => {
      const index = calls.search_reddit.length;
      const input = document.querySelector('#cache-search-input');
      input.value = query;
      document.querySelector('#header-search-form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
      await wait(() => calls.search_reddit.length > index, 'search command');
      return index;
    };
    const typeSearch = async (query) => {
      const index = calls.search_cache.length;
      const input = document.querySelector('#cache-search-input');
      input.value = query;
      input.dispatchEvent(new Event('input',{bubbles:true}));
      await wait(() => calls.search_cache.length > index, 'local cache search command');
      return index;
    };
    const result = () => snapshot([]);
    const controls = () => ({listInert:document.querySelector('#thread-list').inert,sortDisabled:document.querySelector('#post-sort').disabled,submitDisabled:document.querySelector('#header-search-submit').disabled});
    await wait(() => !document.querySelector('#browse-submit').disabled && document.querySelector('.post-tab-open'), 'reader startup and restored tab');
    const data = {};

    // A post-tab operation invalidates the search transition. Its own finalizer must still release the search UI.
    let index = await submitSearch('tab resolve');
    document.querySelector('.post-tab-open').click();
    window.__stub.resolve('search_reddit', index, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'tab-transition search resolve unlock');
    assert(!document.querySelector('#post-sort').disabled && !document.querySelector('#header-search-submit').disabled, 'tab-transition resolve left controls locked');
    data.tabTransitionResolve = 'passed';

    index = await submitSearch('tab reject');
    document.querySelector('.post-tab-open').click();
    window.__stub.reject('search_reddit', index, 'stubbed failure');
    await wait(() => !document.querySelector('#thread-list').inert, 'tab-transition search reject unlock');
    assert(!document.querySelector('#post-sort').disabled && !document.querySelector('#header-search-submit').disabled, 'tab-transition rejection left controls locked');
    data.tabTransitionReject = 'passed';

    // A stale rejection cannot release the newer search's lock.
    const oldIndex = await submitSearch('stale old');
    const newIndex = await submitSearch('current new');
    window.__stub.reject('search_reddit', oldIndex, 'stale rejection');
    await pause();
    assert(document.querySelector('#thread-list').inert && document.querySelector('#post-sort').disabled && document.querySelector('#header-search-submit').disabled, 'stale search released active search lock');
    window.__stub.resolve('search_reddit', newIndex, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'newer search unlock');
    data.staleSearchCannotUnlockNewSearch = 'passed';

    // Search-mode sort options and period control; every search sort launches a fresh native query.
    const sort = document.querySelector('#post-sort');
    const options = Object.fromEntries([...sort.options].map(option => [option.value, option]));
    assert(options.relevance && !options.relevance.hidden && !options.relevance.disabled, 'relevance missing in Reddit search');
    assert(options.comments && !options.comments.hidden && !options.comments.disabled, 'comments missing in Reddit search');
    assert(options.hot.hidden && options.hot.disabled && options.rising.hidden && options.rising.disabled, 'subreddit-only sorts exposed in Reddit search');
    assert(!document.querySelector('#top-period-wrap').hidden && document.querySelector('#top-period-label').textContent === 'Time period', 'search period control is not visible as Time period');
    const start = calls.search_reddit.length;
    for (const value of ['relevance','new','top','comments']) {
      sort.value = value;
      sort.dispatchEvent(new Event('change',{bubbles:true}));
      await wait(() => calls.search_reddit.length > start + ['relevance','new','top','comments'].indexOf(value), `${value} sort rerun`);
    }
    for (let i = start; i < calls.search_reddit.length - 1; i++) window.__stub.resolve('search_reddit', i, result());
    await pause();
    assert(document.querySelector('#thread-list').inert && sort.disabled, 'stale sort rerun released current sort lock');
    window.__stub.resolve('search_reddit', calls.search_reddit.length - 1, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'last sort rerun unlock');
    data.searchSortAndPeriod = 'passed';

    // A subreddit fetch owns browse/input controls; a search can supersede its shared listing lock.
    document.querySelector('#browse-form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    const browseIndex = calls.fetch_subreddit.length - 1;
    await wait(() => calls.fetch_subreddit.length > 0, 'subreddit fetch command');
    assert(options.relevance.hidden && options.comments.hidden && !options.hot.hidden && document.querySelector('#top-period-wrap').hidden, 'subreddit sort options/period scope are wrong');
    const searchIndex = await submitSearch('browse overlap');
    window.__stub.resolve('fetch_subreddit', browseIndex, result());
    await pause();
    assert(!document.querySelector('#browse-submit').disabled && !document.querySelector('#subreddit-input').disabled, 'obsolete browse failed to release its owned controls');
    assert(document.querySelector('#thread-list').inert && sort.disabled, 'obsolete browse released active search listing lock');
    window.__stub.resolve('search_reddit', searchIndex, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'overlapping search unlock');
    data.browseThenSearchOwnership = 'passed';

    // Every input launches local-only FTS; stale FTS replies cannot replace the newest typed query.
    const redditCountBeforeLocalTyping = calls.search_reddit.length;
    const oldCache = await typeSearch('old cached query');
    const latestCache = await typeSearch('latest cached query');
    assert(calls.search_reddit.length === redditCountBeforeLocalTyping, 'typing initiated a Reddit request');
    assert(calls.search_cache[oldCache].args.query === 'old cached query' && calls.search_cache[latestCache].args.query === 'latest cached query', 'local FTS did not track each typed query');
    window.__stub.resolve('search_cache', oldCache, [{id:'t3_stub',title:'Stale cached title',subreddit:'soccer',snippet:'old'}]);
    await pause();
    assert(!document.querySelector('.cache-search-hit'), 'stale local reply rendered over a newer typed query');
    window.__stub.resolve('search_cache', latestCache, [{id:'t3_stub',title:'Latest cached title',subreddit:'soccer',snippet:'latest'}]);
    await wait(() => document.querySelector('.cache-search-hit')?.textContent.includes('Latest cached title'), 'latest local cache result');
    const redditCallsBeforeCacheHit = calls.search_reddit.length;
    const previewCallsBeforeCacheHit = calls.previewFetchCount;
    const cacheHit = document.querySelector('.cache-search-hit');
    cacheHit.focus();
    cacheHit.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));
    await wait(() => document.querySelector('#thread-view').textContent.includes('Stub tab'), 'cached result open');
    assert(calls.fetch_thread.length === 0 && calls.search_reddit.length === redditCallsBeforeCacheHit && calls.previewFetchCount === previewCallsBeforeCacheHit, 'cache-hit Enter used the live search/source/preview path');
    data.latestTypedCacheReplyAndCachedOnlyOpen = 'passed';

    // Enter submits live Reddit search; a late local reply must not reopen or cover its results.
    const pendingCache = await typeSearch('Enter live query');
    assert(calls.search_reddit.length === redditCountBeforeLocalTyping, 'typing initiated a Reddit request before Enter');
    const liveIndex = calls.search_reddit.length;
    document.querySelector('#header-search-form').requestSubmit();
    await wait(() => calls.search_reddit.length > liveIndex, 'Enter live Reddit command');
    assert(document.querySelector('#cache-search-results').hidden, 'Enter did not close local matches');
    assert(calls.search_reddit[liveIndex].args.sort === 'relevance' && calls.search_reddit[liveIndex].args.period === 'all', 'Enter did not use default Reddit search parameters');
    window.__stub.resolve('search_cache', pendingCache, [{id:'t3_stub',title:'Late cached title',subreddit:'soccer',snippet:'late'}]);
    await pause();
    assert(document.querySelector('#cache-search-results').hidden && !document.querySelector('.cache-search-hit'), 'late cache response covered live search');
    window.__stub.resolve('search_reddit', liveIndex, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'Enter Reddit result completion');
    data.enterLiveSearchCancelsLocalReply = 'passed';

    // Starting another input cancels authority of a pending Reddit result without losing local suggestions.
    const oldLive = await submitSearch('old live query');
    const currentCache = await typeSearch('new local query');
    assert(!document.querySelector('#reader-feedback').textContent.includes('Searching Reddit') && !document.querySelector('#connection-status').textContent.startsWith('Searching Reddit'), 'cancelling live search left stale progress text');
    window.__stub.resolve('search_reddit', oldLive, snapshot([thread]));
    await pause();
    const currentRow = {id:'t3_stub',title:'Newest local title',subreddit:'soccer',snippet:'current'};
    window.__stub.resolve('search_cache', currentCache, [currentRow]);
    await wait(() => document.querySelector('.cache-search-hit')?.textContent.includes('Newest local title'), 'new local suggestion after stale Reddit reply');
    assert(!document.querySelector('.browser-heading')?.textContent.includes('old live query'), 'stale live response replaced newer local query context');
    data.newInputCancelsPendingLiveSearch = 'passed';

    // Search sort reruns must also cancel pending FTS results, not just form submission.
    const cacheBeforeSort = await typeSearch('cache before sort');
    const sortSearchIndex = calls.search_reddit.length;
    sort.value = 'new';
    sort.dispatchEvent(new Event('change',{bubbles:true}));
    await wait(() => calls.search_reddit.length > sortSearchIndex, 'sort-triggered Reddit rerun');
    assert(document.querySelector('#cache-search-results').hidden, 'sort rerun did not hide local matches');
    window.__stub.resolve('search_cache', cacheBeforeSort, [{id:'t3_stub',title:'Late sort cache result',subreddit:'soccer'}]);
    await pause();
    assert(document.querySelector('#cache-search-results').hidden, 'late FTS reply reopened dropdown over a sort rerun');
    window.__stub.resolve('search_reddit', sortSearchIndex, result());
    await wait(() => !document.querySelector('#thread-list').inert, 'sort rerun with pending FTS completion');
    data.sortRerunCancelsLocalReply = 'passed';

    // The native acquisition result is the authority for cache feedback. Only two explicit hits count as reuse.
    document.querySelector('#cache-search-clear').click();
    const browseIndexForCache = calls.fetch_subreddit.length;
    document.querySelector('#subreddit-input').value = '/r/soccer';
    document.querySelector('#browse-form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));
    await wait(() => calls.fetch_subreddit.length > browseIndexForCache, 'browse for cache feedback checks');
    window.__stub.resolve('fetch_subreddit', browseIndexForCache, snapshot([thread], {t3_stub:{kind:'video',count:1}}));
    await wait(() => document.querySelector('.thread-open'), 'cache test listing');
    state.validateCachedMedia = false;
    let fetchIndex = calls.fetch_thread.length;
    document.querySelector('.thread-open').click();
    await wait(() => calls.fetch_thread.length > fetchIndex, 'native cache-hit thread fetch');
    const videoUrl = 'http://127.0.0.1:1/' + 'a'.repeat(64) + '/t3_stub/fixture.mp4';
    const threadDownload = (cache) => ({snapshot:snapshot([thread], {t3_stub:{kind:'video',count:1}}),media:[{kind:'video',url:videoUrl,name:'fixture.mp4'}],cache});
    window.__stub.resolve('fetch_thread', fetchIndex, threadDownload({comments_hit:true,media_hit:true}));
    await wait(() => document.querySelector('#connection-status').textContent.includes('Opened from cache'), 'native cache-hit status');
    assert(document.querySelector('#connection-status').textContent.includes('1 local media item'), 'cache-hit status omitted media reuse detail');
    data.nativeThreadCacheHitFeedback = 'passed';

    // Bulk reuse with an existing connected player must leave that DOM node mounted.
    state.validateCachedMedia = true;
    document.querySelector('#tab-browse').click();
    const inlineToggle = document.querySelector('.inline-media-toggle');
    assert(inlineToggle, 'media hint did not expose inline media control');
    inlineToggle.click();
    await wait(() => document.querySelector('[data-inline-media-for="t3_stub"] video'), 'initial inline player');
    const mountedPlayer = document.querySelector('[data-inline-media-for="t3_stub"] video');
    assert(mountedPlayer.isConnected, 'initial video player is detached');
    state.validateCachedMedia = false;
    fetchIndex = calls.fetch_thread.length;
    document.querySelector('#fetch-all-media').click();
    await wait(() => calls.fetch_thread.length > fetchIndex, 'bulk media native cache check');
    window.__stub.resolve('fetch_thread', fetchIndex, threadDownload({comments_hit:true,media_hit:true}));
    await wait(() => document.querySelector('#fetch-all-media-status').textContent.startsWith('Finished'), 'bulk media completion');
    assert(mountedPlayer.isConnected && document.querySelector('[data-inline-media-for="t3_stub"] video') === mountedPlayer, 'native cache reuse replaced the mounted player');
    data.bulkNativeCacheReusePreservesPlayer = 'passed';

    // A native miss or an older mock without cache metadata keeps the download-ready wording.
    for (const [label, cache] of [['miss',{comments_hit:false,media_hit:true}],['legacy',undefined]]) {
      state.validateCachedMedia = false;
      fetchIndex = calls.fetch_thread.length;
      document.querySelector('.thread-open').click();
      await wait(() => calls.fetch_thread.length > fetchIndex, `${label} thread fetch`);
      const download = cache ? threadDownload(cache) : threadDownload(undefined);
      window.__stub.resolve('fetch_thread', fetchIndex, download);
      await wait(() => document.querySelector('#connection-status').textContent.startsWith('Post ready'), `${label} ready status`);
      assert(!document.querySelector('#connection-status').textContent.includes('Opened from cache'), `${label} incorrectly reported cache reuse`);
    }
    data.nativeMissAndLegacyFallback = 'passed';
    return {status:'passed',checks:data};
  };
})();
"""


def run_cli(*args: str, timeout: int = 40) -> str:
    result = subprocess.run([str(PWCLI), "--session", SESSION, *args], text=True, capture_output=True, timeout=timeout)
    output = result.stdout + result.stderr
    if result.returncode:
        raise RuntimeError(f"Playwright CLI failed ({result.returncode}):\n{output}")
    if "Error:" in output or "### Error" in output:
        raise RuntimeError(f"Playwright reported an error:\n{output}")
    return output


def main() -> None:
    if not PWCLI.is_file():
        raise SystemExit(f"Playwright CLI wrapper not found: {PWCLI}; set PWCLI to its path")
    work = pathlib.Path(tempfile.mkdtemp(prefix="hrungnir-search-ui-", dir=ROOT / "var"))
    site = work / "site"
    shutil.copytree(ROOT / "reader", site)
    stub = STUB_JS.replace("__THREAD__", json.dumps(THREAD, separators=(",", ":")))
    index = site / "index.html"
    html = index.read_text()
    html = html.replace('<script defer src="app.js"></script>', f"<script>{stub}</script><script defer src=\"app.js\"></script>")
    index.write_text(html)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]

    class QuietHandler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(site), **kwargs)
        def log_message(self, *_args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), QuietHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        run_cli("open", f"http://127.0.0.1:{port}/index.html")
        run_cli("eval", "localStorage.setItem('offline-reader.preferences.v1', JSON.stringify({tab:'browse',postTabs:[{id:'t3_stub',anchor:'t3_stub',cachedOnly:true}],activePostId:'t3_stub'}))")
        run_cli("reload")
        time.sleep(0.25)
        output = run_cli("eval", "window.__searchUiSmoke()", timeout=60)
        if '"status": "passed"' not in output:
            raise RuntimeError(f"Stub checks did not report pass:\n{output}")
        print(output[output.find("### Result"):].strip())
    finally:
        try:
            run_cli("close", timeout=15)
        except Exception:
            pass
        server.shutdown()
        server.server_close()
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
