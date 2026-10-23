(() => {
  const threads = Array.from({ length: 28 }, (_, i) => ({
    id: `t3_${i === 0 ? 'article' : i === 1 ? 'video' : i === 2 ? 'discussion' : `fixture${i}`}`,
    title: i === 0 ? 'A readable article about the next space mission' : i === 1 ? 'Match highlights with audio' : i === 2 ? 'Season five episode discussion threads' : `Reader fixture ${i}: another story to read`,
    subreddit: i % 2 ? 'soccer' : 'space',
    body: i === 2 ? 'Season five\n\nEpisode one (https://www.reddit.com/r/space/comments/episode/one/)' : '',
    flair: i % 3 === 0 ? 'News' : i % 3 === 1 ? 'Match Clip' : 'Discussion',
    comments: [], missing: [], captured_at: Math.floor(Date.now() / 1000), source: 'proxy', comments_status: 'not_loaded',
    permalink: `https://www.reddit.com/r/${i % 2 ? 'soccer' : 'space'}/comments/${i === 0 ? 'article' : i === 1 ? 'video' : i === 2 ? 'discussion' : `fixture${i}`}/title/`,
    link_url: i === 2 ? 'https://www.reddit.com/r/space/comments/discussion/title/' : i === 1 ? 'https://v.redd.it/fixture/' : `https://example.org/story${i}`,
    thumbnail_url: 'https://b.thumbs.redditmedia.com/fixture.jpg', media_urls: [],
  }));
  const calls = [], posts = new Map(), articles = new Map();
  let library = { items: [], groups: [] };
  const prefs = { tab: 'browse', theme: 'classified', lastSubreddit: 'popular', listings: { popular: threads.map(t => t.id) } };
  if (!localStorage.getItem('offline-reader.preferences.v1')) localStorage.setItem('offline-reader.preferences.v1', JSON.stringify(prefs));
  const hints = { t3_video: { kind: 'video', count: 1 } };
  const snap = (rows = threads) => ({ threads: structuredClone(rows), diagnostics: {}, position: null, media_hints: hints, link_previews: {} });
  const wait = (map, id) => new Promise(resolve => map.set(id, resolve));
  window.__ux = { calls, posts, articles, threads, releasePost: id => posts.get(id)?.(), releaseArticle: id => articles.get(id)?.() };
  window.__TAURI__ = {
    event: { listen: async () => () => {} },
    core: { invoke: async (command, args = {}) => {
      calls.push({ command, args });
      if (command === 'snapshot') return snap();
      if (command === 'library_state') return library;
      if (command === 'get_cache_settings') return { post_cache_hours: 48 };
      if (command === 'take_app_links') return [];
      if (command === 'app_diagnostics') return { report: {} };
      if (command === 'fetch_link_previews') return Object.fromEntries(args.ids.map(id => [id, { kind: id === 't3_video' ? 'video' : 'article', domain: 'example.org', thumbnail: `${location.origin}/${'a'.repeat(64)}/${id}/thumb.png` }]));
      if (command === 'fetch_subreddit') return snap();
      if (command === 'fetch_thread') {
        if (args.include_media !== false) return new Promise(() => {}); // Simulate a stalled media download.
        await wait(posts, args.id);
        const thread = { ...threads.find(t => t.id === args.id), comments_status: 'partial_proxy', comments: [{ id: 't1_parent', parent_id: args.id, body: 'A useful discussion, available before the video finishes.' }] };
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
