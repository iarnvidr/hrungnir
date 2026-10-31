/* Executed only by the dedicated native test app; every IPC call stays real. */
(() => {
  const config = window.__hrungnirUiProbeConfig;
  const started = performance.now();
  const calls = [];
  const checks = [];
  let stage = 'startup';
  let originalInvoke;
  const $ = selector => document.querySelector(selector);
  const rows = () => [...document.querySelectorAll('#thread-list .thread-item')];
  const ids = () => rows().map(row => row.dataset.threadId);
  const clean = value => String(value).replace(/https?:\/\/\S+/g, '[url]').slice(0, 500);
  const emit = payload => window.__TAURI__.event.emit('hrungnir-native-ui-probe', {
    name: 'native-ui', run_id: config.run_id,
    elapsed_ms: Math.round(performance.now() - started), ...payload,
  });
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const wait = async (predicate, description, timeout = 45000) => {
    const deadline = performance.now() + timeout;
    while (performance.now() < deadline) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 40));
    }
    throw new Error(`Timed out: ${description}`);
  };
  const snapshot = () => ({
    heading: $('#current-subreddit')?.textContent,
    query: $('#cache-search-input')?.value,
    scope: $('#search-scope')?.value,
    post_ids: ids(),
    reader_visible: !$('#reader-panel')?.hidden,
    feedback: clean($('#reader-feedback')?.textContent || ''),
    status: clean($('#connection-status')?.textContent || ''),
    comments_header: $('#discussion-comments .comments-header')?.textContent,
    downloaded_comments: document.querySelectorAll('#discussion-comments .comment').length,
  });
  const setInput = (selector, value) => {
    const node = $(selector);
    assert(node, `Missing control ${selector}`);
    node.value = value;
    node.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const setScope = value => {
    const node = $('#search-scope');
    node.value = value;
    node.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const clearSearch = () => {
    const button = $('#cache-search-clear');
    if (button && !button.hidden && !button.disabled) button.click();
    else setInput('#cache-search-input', '');
  };
  const begin = async name => { stage = name; await emit({ stage, finished: false }); };

  async function search(query, scope) {
    await begin(`search:${scope}`);
    clearSearch();
    setScope(scope);
    const smart = $('#search-smart');
    smart.checked = false;
    smart.dispatchEvent(new Event('change', { bubbles: true }));
    const before = calls.length;
    setInput('#cache-search-input', query);
    $('#header-search-form').requestSubmit();
    await wait(() => calls.slice(before).some(call => call.command === 'search_reddit'
      && !call.continuation && call.ok && call.query === query)
      && !$('#header-search-submit').disabled && rows().length > 0,
    `real ${scope} search results`);
    const first = calls.slice(before).find(call => call.command === 'search_reddit'
      && !call.continuation && call.ok && call.query === query);
    assert(new Set(ids()).size === rows().length, 'Search rendered duplicate post IDs');
    if (config.require_paging) {
      assert(first.has_cursor, `${scope} first page exposed no continuation`);
      const hasContinuation = () => calls.slice(before).some(call => call.command === 'search_reddit'
        && call.continuation && call.ok && call.query === query);
      if (!hasContinuation()) {
        const button = $('#search-load-more');
        assert(button && !button.hidden, 'Native search continuation control is unavailable');
        button.click();
      }
      await wait(() => hasContinuation() && rows().length > first.count,
        `new unique ${scope} search results`, 60000);
      assert(new Set(ids()).size === rows().length, 'Continuation rendered duplicate post IDs');
    }
    checks.push({ name: `search:${scope}`, first_count: first.count,
      rendered_count: rows().length, has_cursor: first.has_cursor });
    return snapshot();
  }

  async function run() {
    await wait(() => window.__TAURI__?.core && window.__TAURI_INTERNALS__?.invoke
      && $('#browse-submit') && !$('#browse-submit').disabled, 'native reader startup');
    originalInvoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = function(command, args, ...rest) {
      const tracked = ['fetch_subreddit', 'fetch_listing_page', 'search_reddit',
        'search_cache', 'fetch_thread', 'open_cached_thread'].includes(command);
      const call = tracked ? { command, query: args?.query, id: args?.id,
        subreddit: args?.subreddit, continuation: Boolean(args?.cursor), ok: false } : null;
      return Reflect.apply(originalInvoke, this, [command, args, ...rest]).then(result => {
        if (call) {
          call.ok = true;
          call.count = Array.isArray(result?.threads) ? result.threads.length
            : Array.isArray(result?.posts) ? result.posts.length : Array.isArray(result) ? result.length : null;
          call.has_cursor = Boolean(result?.search_page?.cursor);
          call.comments_hit = result?.cache?.comments_hit;
          call.media_hit = result?.cache?.media_hit;
          calls.push(call);
        }
        return result;
      }, error => { if (call) { call.error = clean(error); calls.push(call); } throw error; });
    };
    await begin('browse:rust');
    $('#tab-browse').click();
    setInput('#subreddit-input', '/r/rust');
    $('#browse-form').requestSubmit();
    await wait(() => calls.some(call => call.command === 'fetch_subreddit' && call.ok)
      && !$('#browse-submit').disabled && rows().length > 0, 'live subreddit UI rows');
    checks.push({ name: 'browse:rust', rendered_count: rows().length });

    await search('rust', 'all');
    await search('borrow checker', 'subreddit');
    await begin('open-search-result');
    const selected = rows()[0];
    const selectedId = selected.dataset.threadId;
    const title = selected.querySelector('.thread-title')?.textContent || 'borrow checker';
    const beforePost = calls.length;
    selected.querySelector('.thread-open').click();
    await wait(() => !$('#reader-panel').hidden
      && $('#discussion-comments .comments-header')
      && calls.slice(beforePost).some(call => call.command === 'fetch_thread' && call.ok),
    'real selected-post comments', 60000);
    checks.push({ name: 'open-search-result', post_id: selectedId,
      downloaded_comments: document.querySelectorAll('#discussion-comments .comment').length });
    $('#tab-browse').click();
    assert(ids().includes(selectedId), 'Returning from a post lost the native search result');
    assert($('#cache-search-input').value === 'borrow checker', 'Post return lost the query');

    await begin('downloaded-search');
    clearSearch();
    setScope('cache');
    const localQuery = (title.match(/[A-Za-z0-9]{3,}/g) || ['borrow', 'checker']).slice(0, 4).join(' ');
    const beforeLocal = calls.length;
    setInput('#cache-search-input', localQuery);
    $('#header-search-form').requestSubmit();
    await wait(() => calls.slice(beforeLocal).some(call => call.command === 'search_cache' && call.ok)
      && !$('#header-search-submit').disabled && ids().includes(selectedId), 'downloaded result in real cache');
    assert(!calls.slice(beforeLocal).some(call => call.command === 'search_reddit'),
      'Downloaded search invoked Reddit search');
    const cachedRow = rows().find(row => row.dataset.threadId === selectedId);
    cachedRow.querySelector('.thread-open').click();
    await wait(() => calls.slice(beforeLocal).some(call => call.command === 'open_cached_thread'
      && call.id === selectedId && call.ok), 'native local-only cached reopen');
    checks.push({ name: 'downloaded-search-and-reopen', post_id: selectedId,
      reddit_search_requests: calls.slice(beforeLocal).filter(call => call.command === 'search_reddit').length });
    return { name: 'native-ui', status: 'ok', finished: true, checks, calls, state: snapshot() };
  }

  run().then(result => emit(result)).catch(error => emit({
    status: 'failed', finished: true, stage, error: clean(error), checks, calls, state: snapshot(),
  })).finally(() => { if (originalInvoke) window.__TAURI_INTERNALS__.invoke = originalInvoke; });
})();
