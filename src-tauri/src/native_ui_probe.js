/* Dedicated native test app only. Controls and native operations stay real. */
(() => {
  const config = window.__hrungnirUiProbeConfig;
  if (!window.__TAURI__?.event || !document.querySelector('#browse-form')
      || window.__hrungnirUiProbeRunId === config.run_id) return;
  window.__hrungnirUiProbeRunId = config.run_id;
  const started = performance.now();
  const checks = [];
  let stage = 'startup';
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
      if (await predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 80));
    }
    throw new Error(`Timed out: ${description}`);
  };
  // The reader captures its bound invoke function at startup. Observe actual
  // native receipts instead of replacing or mocking that function.
  const diagnostics = () => window.__TAURI__.core.invoke('app_diagnostics');
  const mark = report => Math.max(0, ...(report.operations || []).map(op => op.timestamp_ms || 0));
  const completed = (report, name, after) => (report.operations || []).some(op =>
    op.stage === name && op.status === 'complete' && op.timestamp_ms > after);
  const requests = report => report.proxy?.response_stats?.requests || 0;
  const snapshot = () => ({
    heading: $('#current-subreddit')?.textContent,
    query: $('#cache-search-input')?.value, scope: $('#search-scope')?.value,
    post_ids: ids(), reader_visible: !$('#reader-panel')?.hidden,
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
    $('#search-scope').value = value;
    $('#search-scope').dispatchEvent(new Event('change', { bubbles: true }));
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
    $('#search-smart').checked = false;
    $('#search-smart').dispatchEvent(new Event('change', { bubbles: true }));
    const before = mark(await diagnostics());
    setInput('#cache-search-input', query);
    $('#header-search-form').requestSubmit();
    await wait(async () => completed(await diagnostics(), 'search', before)
      && !$('#header-search-submit').disabled && rows().length > 0,
    `real ${scope} search results`);
    const firstCount = rows().length;
    const firstRow = rows()[0];
    assert(new Set(ids()).size === rows().length, 'Search rendered duplicate post IDs');
    if (config.require_paging) {
      const button = $('#search-load-more');
      assert(button && !button.hidden, `${scope} first page exposed no continuation`);
      if (button.getAttribute('aria-disabled') !== 'true') button.click();
      await wait(() => rows().length > firstCount, `new unique ${scope} search results`, 60000);
      assert(rows()[0] === firstRow, 'Search continuation replaced an existing row');
      assert(new Set(ids()).size === rows().length, 'Continuation rendered duplicate post IDs');
    }
    checks.push({ name: `search:${scope}`, first_count: firstCount, rendered_count: rows().length });
  }

  async function run() {
    await begin('startup');
    await wait(() => window.__TAURI__?.core && $('#browse-submit')
      && !$('#browse-submit').disabled, 'native reader startup');
    await begin('browse:rust');
    const beforeBrowse = mark(await diagnostics());
    $('#tab-browse').click();
    setInput('#subreddit-input', '/r/rust');
    $('#browse-form').requestSubmit();
    await wait(async () => completed(await diagnostics(), 'listing', beforeBrowse)
      && !$('#browse-submit').disabled && rows().length > 0, 'live subreddit UI rows');
    const firstBrowseCount = rows().length;
    const firstBrowseRow = rows()[0];
    const browseMore = $('#listing-load-more');
    assert(browseMore && !browseMore.hidden, 'Subreddit continuation control is unavailable');
    if (browseMore.getAttribute('aria-disabled') !== 'true') browseMore.click();
    await wait(() => rows().length > firstBrowseCount, 'additional native subreddit rows', 60000);
    assert(rows()[0] === firstBrowseRow, 'Subreddit continuation replaced an existing row');
    assert(new Set(ids()).size === rows().length, 'Subreddit continuation rendered duplicates');
    checks.push({ name: 'browse:rust', first_count: firstBrowseCount, rendered_count: rows().length });

    await search('rust', 'all');
    await search('borrow checker', 'subreddit');
    await begin('open-search-result');
    const selected = rows()[0];
    const selectedId = selected.dataset.threadId;
    const title = selected.querySelector('.thread-title')?.textContent || 'borrow checker';
    selected.querySelector('.thread-open').click();
    await wait(async () => {
      const stored = await window.__TAURI__.core.invoke('snapshot');
      const post = stored.threads?.find(thread => thread.id === selectedId);
      return post && post.comments_status !== 'not_loaded' && post.comments?.length > 0
        && !$('#reader-panel').hidden && $('#discussion-comments .comment')
        && !$('#discussion-comments .cached-listing-note');
    },
      'real selected-post discussion', 60000);
    checks.push({ name: 'open-search-result', post_id: selectedId,
      downloaded_comments: document.querySelectorAll('#discussion-comments .comment').length });
    $('#tab-browse').click();
    assert(ids().includes(selectedId), 'Returning from a post lost the native search result');
    assert($('#cache-search-input').value === 'borrow checker', 'Post return lost the query');

    await begin('downloaded-search');
    clearSearch();
    setScope('cache');
    const localQuery = (title.match(/[A-Za-z0-9]{3,}/g) || ['borrow', 'checker']).slice(0, 4).join(' ');
    const beforeLocal = await diagnostics();
    setInput('#cache-search-input', localQuery);
    $('#header-search-form').requestSubmit();
    await wait(() => !$('#header-search-submit').disabled && ids().includes(selectedId)
      && $('#cache-search-input').value === localQuery
      && $('#current-subreddit').textContent.toLowerCase().includes('downloaded'),
    'downloaded result in real cache');
    const cachedRow = rows().find(row => row.dataset.threadId === selectedId);
    cachedRow.querySelector('.thread-open').click();
    await wait(() => !$('#reader-panel').hidden && $('#discussion-comments .comments-header'),
      'native local-only cached reopen');
    const afterLocal = await diagnostics();
    assert(requests(afterLocal) === requests(beforeLocal), 'Downloaded search or cached reopen made a Reddit request');
    assert(!completed(afterLocal, 'search', mark(beforeLocal)), 'Downloaded search started Reddit search');
    checks.push({ name: 'downloaded-search-and-reopen', post_id: selectedId,
      reddit_requests: requests(afterLocal) - requests(beforeLocal) });
    return { name: 'native-ui', status: 'ok', finished: true, checks, state: snapshot() };
  }

  run().then(result => emit(result)).catch(error => emit({
    status: 'failed', finished: true, stage, error: clean(error), checks, state: snapshot(),
  }));
})();
