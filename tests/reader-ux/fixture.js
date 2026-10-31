(() => {
  const threads = Array.from({ length: 28 }, (_, i) => ({
    id: `t3_${i === 0 ? 'article' : i === 1 ? 'video' : i === 2 ? 'discussion' : `fixture${i}`}`,
    title: i === 0 ? 'A readable article about the next space mission' : i === 1 ? 'Match highlights with audio' : i === 2 ? 'Season five episode discussion threads' : `Reader fixture ${i}: another story to read`,
    subreddit: i % 2 ? 'soccer' : 'space',
    comment_count: i + 3,
    body: i === 2 ? 'Season five\n\nEpisode one (https://www.reddit.com/r/space/comments/episode/one/)' : '',
    flair: i % 3 === 0 ? 'News' : i % 3 === 1 ? 'Match Clip' : 'Discussion',
    comments: [], missing: [], captured_at: Math.floor(Date.now() / 1000), source: 'proxy', comments_status: 'not_loaded',
    permalink: `https://www.reddit.com/r/${i % 2 ? 'soccer' : 'space'}/comments/${i === 0 ? 'article' : i === 1 ? 'video' : i === 2 ? 'discussion' : `fixture${i}`}/title/`,
    link_url: i === 2 ? 'https://www.reddit.com/r/space/comments/discussion/title/' : i === 1 ? 'https://v.redd.it/fixture/' : `https://example.org/story${i}`,
    thumbnail_url: 'https://b.thumbs.redditmedia.com/fixture.jpg', media_urls: [],
  }));
  const calls = [], completed = [], posts = new Map(), articles = new Map();
  let library = { items: [], groups: [] };
  const prefs = { tab: 'browse', theme: 'classified', lastSubreddit: 'popular', listings: { popular: threads.map(t => t.id) } };
  if (!localStorage.getItem('offline-reader.preferences.v1')) localStorage.setItem('offline-reader.preferences.v1', JSON.stringify(prefs));
  const hints = { t3_video: { kind: 'video', count: 1 } };
  const snap = (rows = threads) => ({ threads: structuredClone(rows), diagnostics: {}, position: null, media_hints: hints, link_previews: {} });
  const wait = (map, id) => new Promise(resolve => map.set(id, resolve));
  const listeners = new Map();
  const updates = { available: null, failCheck: false, failInstall: false };
  const search = {
    rows: threads, hits: [], error: false, comments: null,
    failCommunity: null, failAllRedirect: false, holdCommunityFor: null, completedCommunities: [],
    cacheError: false, holdCache: false, completedCacheQueries: [],
      enablePaging: false, pageError: false, pageExpired: false, holdPage: false, pageCalls: [], repeatCursor: false, pageThreads: [],
      refreshError: false, refreshNotLoaded: false, refreshHold: false, refreshedComments: null, refreshedMediaUrls: null, provideMedia: false,
      streamCandidateCalls: [], streamPlayCalls: [], streamPlayRejectOnce: false,
  };
  const pagedThreads = threads.map(thread => ({ ...thread, subreddit: 'space' }));
  window.__ux = { calls, completed, posts, articles, threads, updates, search,
    emit: (name, payload) => listeners.get(name)?.({payload}),
    releasePost: id => posts.get(id)?.(), releaseArticle: id => articles.get(id)?.() };
  const nativeCanPlayType = HTMLMediaElement.prototype.canPlayType;
  HTMLMediaElement.prototype.canPlayType = function (type) {
    return String(type).includes('mpegurl') ? 'maybe' : nativeCanPlayType.call(this, type);
  };
  HTMLMediaElement.prototype.play = function () {
    search.streamPlayCalls.push(this);
    if (search.streamPlayRejectOnce) {
      search.streamPlayRejectOnce = false;
      const error = new Error('Playback requires a direct gesture');
      error.name = 'NotAllowedError';
      return Promise.reject(error);
    }
    this.dispatchEvent(new Event('playing'));
    return Promise.resolve();
  };
  const mediaSrc = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  Object.defineProperty(HTMLMediaElement.prototype, 'src', {
    configurable: true,
    get() { return this.dataset.uxStreamSrc || mediaSrc.get.call(this); },
    set(value) {
      if (this.getAttribute('aria-label') === 'Reddit video stream player') this.dataset.uxStreamSrc = String(value);
      else mediaSrc.set.call(this, value);
    },
  });
  window.__TAURI__ = {
    event: { listen: async (name, callback) => { listeners.set(name, callback); return () => listeners.delete(name); } },
    core: { invoke: async (command, args = {}) => {
      calls.push({ command, args });
      if (command === 'app_update_version') return '0.6.9';
      if (command === 'check_for_updates') {
        if (updates.holdCheck) await new Promise(resolve => { updates.releaseCheck = resolve; });
        if (updates.failCheck) throw new Error('Network unavailable');
        return { current_version: '0.6.9', available: updates.available };
      }
      if (command === 'install_update') {
        if (updates.holdInstall) await new Promise(resolve => { updates.releaseInstall = resolve; });
        if (updates.failInstall) throw new Error('Signature verification failed');
        return null;
      }
      if (command === 'restart_updated_app') return null;
      if (command === 'snapshot') return snap();
      if (command === 'video_stream_candidate') {
        search.streamCandidateCalls.push({ ...args });
        return 'https://v.redd.it/fixture/HLSPlaylist.m3u8';
      }
      if (command === 'search_reddit') {
        if (search.hold) await new Promise(resolve => { search.release = resolve; });
        if (search.error) throw new Error('Search source unavailable');
        return snap(args.subreddit ? search.rows.filter(thread => thread.subreddit === args.subreddit) : search.rows);
      }
      if (command === 'search_cache') {
        const rows = structuredClone(search.hits);
        if (search.holdCache) await new Promise(resolve => { search.releaseCache = resolve; });
        if (search.cacheError) throw new Error('Downloaded cache is unavailable');
        search.completedCacheQueries.push({query: args.query, subreddit: args.subreddit});
        return rows;
      }
      if (command === 'open_cached_thread') {
        const thread = { ...threads.find(thread => thread.id === args.id), comments_status: 'partial_proxy', comments: [{id: 't1_parent', parent_id: args.id, body: 'A cached comment match.'}] };
        return {snapshot: snap([thread]), media: [], cache: {comments_hit: true, media_hit: true}};
      }
      if (command === 'library_state') return library;
      if (command === 'get_cache_settings') return { post_cache_hours: 48 };
      if (command === 'take_app_links') return [];
      if (command === 'app_diagnostics') return { report: {} };
      if (command === 'fetch_link_previews') return Object.fromEntries(args.ids.map(id => [id, { kind: id === 't3_video' ? 'video' : 'article', domain: 'example.org', thumbnail: `${location.origin}/${'a'.repeat(64)}/${id}/thumb.png` }]));
      if (command === 'fetch_subreddit') {
        if (args.subreddit === 'all' && search.failAllRedirect) throw new Error('Request to /r/all redirected to a different feed');
        if (search.failCommunity === args.subreddit) throw new Error('Community source unavailable');
        if (search.holdCommunity || search.holdCommunityFor === args.subreddit) await new Promise(resolve => { search.releaseCommunity = resolve; });
        search.completedCommunities.push(args.subreddit);
        if (search.enablePaging) {
          search.pageThreads = pagedThreads.map(thread => ({ ...thread, subreddit: args.subreddit }));
          return { ...snap(search.pageThreads.slice(0, 25)), listing_page: { cursor: 'page-1', exhausted: false } };
        }
        return snap();
      }
      if (command === 'fetch_listing_page') {
        search.pageCalls.push({ ...args });
        if (search.holdPage) await new Promise(resolve => { search.releasePage = resolve; });
        if (search.pageExpired) throw new Error('Listing cursor expired; refresh the listing.');
        if (search.pageError) throw new Error('Continuation source unavailable');
        if (args.cursor === 'page-1') return {
          posts: [
            { ...search.pageThreads[0], body: '', comments: [], missing: [], comments_status: 'not_loaded', comment_count: 30 },
            search.pageThreads[24], search.pageThreads[25], search.pageThreads[26],
          ],
          listing_page: { cursor: search.repeatCursor ? 'page-1' : 'page-2', exhausted: false },
        };
        if (args.cursor === 'page-2') return {
          posts: [search.pageThreads[26], search.pageThreads[27]],
          media_hints: { t3_fixture27: { kind: 'video', count: 1 } },
          listing_page: { cursor: null, exhausted: true },
        };
        throw new Error('Unknown fixture cursor');
      }
      if (command === 'fetch_thread') {
        if (args.include_media !== false) {
          if (!search.provideMedia) return new Promise(() => {}); // Simulate a stalled media download.
          const thread = { ...threads.find(t => t.id === args.id), comments_status: 'partial_proxy', comments: search.comments || [] };
          return { snapshot: snap([thread]), media: [{ kind: 'video', name: 'Fixture clip', url: `https://asset.localhost/${'a'.repeat(64)}/${args.id}/clip.mp4` }], cache: { comments_hit: true, media_hit: true } };
        }
        if (args.refresh) {
          if (search.refreshHold) await new Promise(resolve => { search.releaseRefresh = resolve; });
          if (search.refreshNotLoaded) {
            const thread = { ...threads.find(t => t.id === args.id), comments_status: 'not_loaded', comments: [], missing: [] };
            return { snapshot: snap([thread]), media: [], cache: { comments_hit: false, media_hit: false } };
          }
          if (search.refreshError) throw new Error('Reddit comments are unavailable');
          const thread = { ...threads.find(t => t.id === args.id), media_urls: search.refreshedMediaUrls || threads.find(t => t.id === args.id).media_urls, comment_count: 8, comments_status: 'partial_proxy', comments: search.refreshedComments || [
            { id: 't1_parent', parent_id: args.id, body: 'Updated parent comment.' },
            { id: 't1_reply', parent_id: 't1_parent', body: 'A newly downloaded reply.' },
          ] };
          return { snapshot: snap([thread]), media: [], cache: { comments_hit: false, media_hit: true } };
        }
        await wait(posts, args.id);
        completed.push(args.id);
        const thread = { ...threads.find(t => t.id === args.id), comments_status: 'partial_proxy', comments: search.comments || [{ id: 't1_parent', parent_id: args.id, body: 'A useful discussion, available before the video finishes.' }] };
        return { snapshot: snap([thread]), media: [], cache: { comments_hit: false, media_hit: false } };
      }
      if (command === 'fetch_link_article') {
        await wait(articles, args.id);
        return { title: 'The next space mission', body: Array.from({length: 16}, (_, i) => `Paragraph ${i + 1}. A synthetic article used to check stable expansion, readable line length and shared article rendering. This content belongs in both the listing and the opened post.`).join('\n\n'), url: 'https://example.org/story', cached: false };
      }
      if (command === 'save_library_post') {
        const thread = threads.find(t => t.id === args.id);
        library = { ...library, items: [{ id: args.id, kind: 'post', collection: args.collection, keep_forever: args.keep_forever, thread, media: [] }] };
        return library;
      }
      if (command === 'remove_library_item') { library = { ...library, items: [] }; return library; }
      if (command === 'validate_cached_media') return false;
      return null;
    } },
  };
})();
