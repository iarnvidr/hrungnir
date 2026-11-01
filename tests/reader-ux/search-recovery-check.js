async (page) => {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const waitForListing = async name => {
    await page.locator('#subreddit-input').fill(name);
    await page.locator('#browse-submit').click();
    await page.waitForFunction(() => !document.querySelector('#browse-submit').disabled);
  };
  const openSettings = async () => {
    if (!(await page.locator('.header-tools').evaluate(node => node.open))) await page.locator('.header-tools > summary').click();
    await page.locator('#settings-open').click();
  };

  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28);
  await waitForListing('soccer');
  assert(await page.locator('.thread-item[data-thread-id="t3_article"] .thread-comment-count').innerText() === '3 comments', 'post row does not show the source comment total');
  const originalCount = await page.locator('.thread-item').count();

  // A failed community fetch leaves the previously rendered listing in place.
  await page.evaluate(() => { window.__ux.search.failCommunity = 'space'; });
  await page.locator('#subreddit-input').fill('space');
  await page.locator('#browse-submit').click();
  await page.waitForFunction(() => document.querySelector('#reader-feedback').textContent.includes('Could not fetch /r/space'));
  assert((await page.locator('#current-subreddit').innerText()).includes('r/soccer'), 'failed subreddit fetch replaced the current community');
  assert(await page.locator('.thread-item').count() === originalCount, 'failed subreddit fetch removed the current posts');
  assert(await page.locator('#browse-submit').isEnabled() && await page.locator('#subreddit-input').isEnabled(), 'failed fetch left controls disabled');
  assert(await page.locator('#subreddit-input').evaluate(node => node === document.activeElement), 'failed fetch did not return focus to the subreddit field');
  assert(await page.locator('#reader-feedback .retry-action').isVisible(), 'failed fetch has no retry action');

  // Retry keeps the requested community and commits only after success.
  await page.evaluate(() => { window.__ux.search.failCommunity = null; });
  await page.locator('#reader-feedback .retry-action').click();
  await page.waitForFunction(() => document.querySelector('#current-subreddit').textContent.includes('r/space') && !document.querySelector('#browse-submit').disabled);
  assert((await page.locator('#current-subreddit').innerText()).includes('r/space'), 'retry did not commit the requested community');

  // r/all is attempted, explained precisely when the backend rejects its redirect,
  // and offers search only as an explicit user-selected next step.
  await page.evaluate(() => { window.__ux.search.failAllRedirect = true; });
  await page.locator('#subreddit-input').fill('all');
  await page.locator('#browse-submit').click();
  await page.waitForFunction(() => document.querySelector('#reader-feedback').textContent.includes('r/all redirects to a different feed'));
  assert((await page.locator('#current-subreddit').innerText()).includes('r/space'), 'r/all failure changed the current listing');
  assert((await page.locator('#reader-feedback').innerText()).includes('does not follow that redirect'), 'r/all redirect behavior is unexplained');
  assert(!(await page.locator('#reader-feedback').innerText()).toLowerCase().includes('certificate'), 'r/all redirect suggested unrelated trust recovery');
  const searchesBeforeAlternative = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit').length);
  await page.locator('#reader-feedback .feedback-action').click();
  assert(await page.locator('#search-scope').inputValue() === 'all', 'search alternative did not select the all-Reddit scope');
  assert(await page.locator('#cache-search-input').evaluate(node => node === document.activeElement), 'search alternative did not focus the query field');
  assert(await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit').length) === searchesBeforeAlternative, 'search ran before the user entered a query');
  await page.locator('#cache-search-input').fill('wildlife');
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => document.querySelector('#current-subreddit').textContent.includes('Reddit results for'));
  const globalSearch = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit').at(-1));
  assert(globalSearch.args.subreddit === null, 'Reddit search alternative was not global');

  // A late subreddit result must not replace a newer search view.
  await page.locator('#cache-search-clear').click();
  await page.evaluate(() => { window.__ux.search.holdCommunityFor = 'soccer'; });
  await page.locator('#subreddit-input').fill('soccer');
  await page.locator('#browse-submit').click();
  await page.waitForFunction(() => typeof window.__ux.search.releaseCommunity === 'function');
  await page.locator('#cache-search-input').fill('fresh query');
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => document.querySelector('#current-subreddit').textContent.includes('Reddit results for “fresh query”'));
  await page.evaluate(() => { window.__ux.search.holdCommunityFor = null; window.__ux.search.releaseCommunity(); });
  await page.waitForFunction(() => !document.querySelector('#browse-submit').disabled);
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 60)));
  assert((await page.locator('#current-subreddit').innerText()).includes('Reddit results for “fresh query”'), 'late subreddit response replaced a newer search');

  // Leaving the browse view while a community request is held keeps that
  // navigation choice and releases the abandoned list's busy state on return.
  await page.evaluate(() => { window.__ux.search.holdCommunityFor = 'space'; });
  await page.locator('#subreddit-input').fill('space');
  await page.locator('#browse-submit').click();
  await page.waitForFunction(() => typeof window.__ux.search.releaseCommunity === 'function');
  await page.locator('#tab-library').click();
  await page.waitForFunction(() => !document.querySelector('#library-panel').hidden);
  await page.evaluate(() => { window.__ux.search.holdCommunityFor = null; window.__ux.search.releaseCommunity(); });
  await page.waitForFunction(() => window.__ux.search.completedCommunities.includes('space'));
  await page.waitForFunction(() => !document.querySelector('#thread-list').hasAttribute('aria-busy'));
  assert(await page.locator('#library-panel').isVisible(), 'late subreddit completion pulled the user away from Library');

  // Downloaded suggestion errors have a retry, retain the query, and do not
  // steal focus when the asynchronous failure arrives.
  await page.locator('#cache-search-clear').click();
  await page.evaluate(() => { window.__ux.search.cacheError = true; });
  await page.locator('#cache-search-input').fill('arctic fox');
  await page.waitForFunction(() => document.querySelector('#cache-search-status').textContent.includes('Downloaded search failed'));
  assert(await page.locator('#cache-search-retry').isVisible(), 'downloaded search failure has no retry button');
  assert(await page.locator('#cache-search-input').inputValue() === 'arctic fox', 'downloaded search failure lost the query');
  assert(await page.locator('#cache-search-input').evaluate(node => node === document.activeElement), 'background cache failure stole typing focus');
  const redditCallsBeforeRetry = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit').length);
  await page.evaluate(() => { window.__ux.search.cacheError = false; window.__ux.search.hits = [{id:'t3_article', title:'Arctic fox notes', subreddit:'space', snippet:'Saved locally'}]; });
  await page.locator('#cache-search-retry').click();
  await page.waitForFunction(() => document.querySelector('#cache-search-list .cache-search-title')?.textContent === 'Arctic fox notes');
  assert((await page.locator('#cache-search-status').innerText()).includes('Downloaded matches on this device'), 'suggestion source is not visible');
  assert(await page.locator('#cache-search-input').inputValue() === 'arctic fox', 'retry changed the downloaded search query');
  assert(await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit').length) === redditCallsBeforeRetry, 'downloaded retry contacted Reddit');

  // A response for the same query under an older community scope cannot replace
  // results from the newly selected all-Reddit scope.
  await page.locator('#cache-search-clear').click();
  await page.locator('#search-scope').selectOption('subreddit');
  await page.evaluate(() => {
    window.__ux.search.hits = [{id:'t3_video', title:'Old scoped result', subreddit:'soccer'}];
    window.__ux.search.holdCache = true;
  });
  await page.locator('#cache-search-input').fill('same query');
  await page.waitForFunction(() => typeof window.__ux.search.releaseCache === 'function');
  await page.evaluate(() => {
    window.__ux.search.holdCache = false;
    window.__ux.search.hits = [{id:'t3_article', title:'All Reddit downloaded result', subreddit:'space'}];
  });
  await page.locator('#search-scope').focus();
  await page.locator('#search-scope').selectOption('all');
  await page.waitForFunction(() => document.querySelector('#cache-search-list .cache-search-title')?.textContent === 'All Reddit downloaded result');
  assert(await page.locator('#search-scope').evaluate(node => node === document.activeElement), 'scope change lost focus while suggestions loaded');
  const cacheCalls = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_cache').slice(-2));
  assert(cacheCalls[0].args.subreddit === 'space' && cacheCalls[1].args.subreddit === null, 'cache search did not preserve its requested scope');
  await page.evaluate(() => window.__ux.search.releaseCache());
  await page.waitForFunction(() => window.__ux.search.completedCacheQueries.filter(item => item.query === 'same query').length === 2);
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 50)));
  assert(await page.locator('#cache-search-list .cache-search-title').innerText() === 'All Reddit downloaded result', 'late cache response replaced results from a newer scope');

  // Pagination keeps the current page intact on errors, retries the same cursor,
  // deduplicates overlapping pages, and continues when the sentinel is reached.
  await page.evaluate(() => {
    window.__ux.search.enablePaging = true;
    window.__ux.search.pageError = true;
    window.__ux.search.holdPage = true;
    window.scrollTo(0, 0);
  });
  await waitForListing('space');
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25);
  assert(await page.locator('.thread-item[data-thread-id="t3_article"] .thread-comment-count').innerText() === '3 comments', 'post row does not show the source comment total');
  await page.locator('.thread-open').first().click();
  await page.waitForFunction(() => window.__ux.posts.has('t3_article'));
  await page.evaluate(() => window.__ux.releasePost('t3_article'));
  await page.waitForFunction(() => document.querySelector('.comments-header')?.textContent === '3 comments · 1 downloaded');
  assert((await page.locator('.partial-comments-note').innerText()).includes('Downloaded 1 of 3 comments'), 'partial discussion does not explain the loaded-vs-source count');
  assert(!(await page.locator('#thread-view').innerText()).includes('No comments yet'), 'partial discussion claimed no comments exist');
  await page.locator('#tab-browse').click();
  if (!(await page.evaluate(() => window.__ux.search.pageCalls.length))) await page.locator('#listing-load-more').click();
  await page.waitForFunction(() => typeof window.__ux.search.releasePage === 'function');
  await page.evaluate(() => { window.scrollTo(0, 0); window.__ux.search.releasePage(); });
  await page.waitForFunction(() => document.querySelector('#listing-pagination-status').textContent.includes('Could not load more posts'));
  assert(await page.locator('.thread-item').count() === 25, 'pagination failure removed existing posts');
  assert((await page.locator('#current-subreddit').innerText()).includes('Posts in r/space'), 'pagination failure changed the selected listing');
  assert(await page.evaluate(() => window.scrollY) === 0, 'pagination failure changed the user scroll position');
  assert(await page.locator('#listing-load-more').innerText() === 'Retry loading more posts', 'pagination failure has no retry action');
  await page.evaluate(() => { window.__ux.search.pageError = false; });
  await page.locator('#listing-load-more').click();
  await page.waitForFunction(() => window.__ux.search.pageCalls.length === 2 && typeof window.__ux.search.releasePage === 'function');
  assert(await page.evaluate(() => window.__ux.search.pageCalls[0].cursor === window.__ux.search.pageCalls[1].cursor), 'pagination retry advanced or replaced the failed cursor');
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.locator('.thread-item[data-thread-id="t3_article"] .thread-open').click();
  await page.waitForFunction(() => document.querySelector('#thread-view')?.textContent.includes('A readable article about the next space mission'));
  if (await page.evaluate(() => window.__ux.posts.has('t3_article'))) await page.evaluate(() => window.__ux.releasePost('t3_article'));
  await page.waitForFunction(() => !document.querySelector('.thread-item[data-thread-id="t3_article"]')?.getAttribute('aria-busy')?.includes('true')
    && document.querySelector('#discussion-comments')?.textContent.includes('A useful discussion, available before the video finishes.'));
  await page.evaluate(() => { window.__ux.firstListingRow = document.querySelector('.thread-item[data-thread-id="t3_article"]'); });
  await page.evaluate(() => window.__ux.search.releasePage());
  assert(await page.locator('#thread-view').isVisible(), 'page completion pulled the reader away from the selected post');
  assert((await page.locator('#discussion-comments').innerText()).includes('A useful discussion, available before the video finishes.'), 'page summary replaced downloaded comments while the reader was open');
  await page.locator('#tab-browse').click();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 27);
  assert(await page.evaluate(() => document.querySelector('.thread-item[data-thread-id="t3_article"]') === window.__ux.firstListingRow), 'returning from the reader remounted existing feed rows after append');
  assert(await page.locator('.thread-item[data-thread-id="t3_article"] .thread-comment-count').innerText() === '30 comments', 'listing summary did not refresh the existing post source comment total');
  assert(await page.evaluate(() => window.scrollY) === 0, 'appending a page moved the current scroll position');
  await page.locator('.thread-item[data-thread-id="t3_article"] .thread-open').click();
  await page.waitForFunction(() => document.querySelector('.comments-header')?.textContent === '30 comments · 1 downloaded');
  assert((await page.locator('#discussion-comments').innerText()).includes('A useful discussion, available before the video finishes.'), 'summary update discarded the previously downloaded comment body');
  await page.locator('#tab-browse').click();
  assert((await page.locator('#listing-pagination-status').innerText()).includes('Scroll to load more'), 'continuation state is not announced');
  await page.evaluate(() => document.querySelector('#listing-sentinel').scrollIntoView({block:'center'}));
  await page.waitForFunction(() => window.__ux.search.pageCalls.length === 3 && window.__ux.search.pageCalls[2].cursor === 'page-2' && typeof window.__ux.search.releasePage === 'function');
  const scrollAtSecondPage = await page.evaluate(() => window.scrollY);
  await page.evaluate(() => window.__ux.search.releasePage());
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28 && document.querySelector('#listing-pagination-status').textContent.includes('End of this listing'));
  assert(await page.evaluate(() => window.scrollY) === scrollAtSecondPage, 'automatic continuation moved the user scroll position');
  assert(await page.locator('.thread-item[data-thread-id="t3_fixture24"]').count() === 1, 'overlapping pages duplicated an existing post');
  assert(await page.evaluate(() => document.querySelectorAll('.thread-item').length === new Set([...document.querySelectorAll('.thread-item[data-thread-id]')].map(node => node.dataset.threadId)).size), 'pagination produced duplicate post rows');
  assert(!(await page.locator('#listing-load-more').isVisible()), 'end of listing left a load button enabled');
  assert(await page.locator('.thread-item[data-thread-id="t3_fixture27"] .inline-media-toggle').isVisible(), 'continued video post lost its media affordance');
  assert(!(await page.evaluate(() => window.__ux.calls.some(call => call.command === 'fetch_thread' && call.args.id === 't3_fixture27'))), 'continuation fetched video media before the user asked to play it');
  await page.waitForFunction(() => window.__ux.search.streamCandidateCalls.some(call => call.id === 't3_fixture27') && document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .inline-media-toggle')?.textContent === 'Play video');
  assert(await page.evaluate(() => window.__ux.search.streamPlayCalls.length === 0), 'video began playing before the user clicked Play');
  await page.evaluate(() => { window.__ux.search.provideMedia = true; window.__ux.search.streamPlayRejectOnce = true; });
  await page.locator('.thread-item[data-thread-id="t3_fixture27"] .inline-media-toggle').click();
  await page.waitForFunction(() => window.__ux.calls.some(call => call.command === 'fetch_thread' && call.args.id === 't3_fixture27'));
  await page.waitForFunction(() => document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .streaming-media-status')?.textContent.includes('saved for offline'));
  assert(await page.evaluate(() => {
    const player = document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .streaming-media-panel video');
    window.__ux.streamPlayer = player;
    return Boolean(player && player.src === 'https://v.redd.it/fixture/HLSPlaylist.m3u8' && window.__ux.search.streamPlayCalls.includes(player));
  }), 'Play did not start the validated Reddit stream from the click gesture');
  assert((await page.locator('.thread-item[data-thread-id="t3_fixture27"] .streaming-media-status').innerText()).includes('press Play in the player'), 'blocked automatic playback hid the controls player instead of inviting a direct Play');
  assert(await page.evaluate(() => document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .streaming-media-panel video') === window.__ux.streamPlayer), 'NotAllowedError removed the player before native controls could be used');
  await page.evaluate(() => window.__ux.streamPlayer.play());
  assert(await page.evaluate(() => document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .streaming-media-panel video') === window.__ux.streamPlayer), 'offline media completion replaced the active stream player');
  await page.evaluate(() => window.__ux.streamPlayer.dispatchEvent(new Event('error')));
  await page.waitForFunction(() => document.querySelector('.thread-item[data-thread-id="t3_fixture27"] .media-section video'));
  assert(await page.locator('.thread-item[data-thread-id="t3_fixture27"] .media-section video').getAttribute('src') !== 'https://v.redd.it/fixture/HLSPlaylist.m3u8', 'stream error did not fall back to the downloaded local video');

  // Refreshing comments updates cached text/counts without remounting the
  // current video player or discarding the prior discussion on failure.
  await page.evaluate(() => { window.__ux.search.refreshNotLoaded = true; window.__ux.search.refreshError = false; });
  await page.locator('.thread-item[data-thread-id="t3_video"] .thread-open').click();
  await page.waitForFunction(() => window.__ux.posts.has('t3_video'));
  await page.evaluate(() => window.__ux.releasePost('t3_video'));
  await page.waitForFunction(() => document.querySelector('#thread-view video'));
  await page.evaluate(() => {
    window.__ux.videoBeforeRefresh = document.querySelector('#thread-view video');
    window.__ux.videoSourceBeforeRefresh = window.__ux.videoBeforeRefresh.currentSrc || window.__ux.videoBeforeRefresh.src;
    return window.__ux.videoSourceBeforeRefresh;
  });
  const oldDiscussion = await page.locator('#discussion-comments').innerText();
  await page.locator('.discussion-refresh').focus();
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.querySelector('#discussion-comments .discussion-refresh-status')?.textContent.includes('Could not refresh comments'));
  assert(await page.locator('.discussion-refresh').evaluate(node => node === document.activeElement), 'invalid listing-only refresh response dropped keyboard focus');
  assert((await page.locator('#discussion-comments').innerText()).includes(oldDiscussion.split('\n')[0]), 'listing-only refresh response replaced the cached discussion');
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.videoBeforeRefresh), 'listing-only refresh response remounted the video player');
  await page.evaluate(() => { window.__ux.search.refreshNotLoaded = false; window.__ux.search.refreshError = true; });
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.querySelector('#discussion-comments .discussion-refresh-status')?.textContent.includes('Could not refresh comments'));
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.videoBeforeRefresh), 'failed discussion refresh remounted the video player');
  assert(await page.evaluate(() => (document.querySelector('#thread-view video').currentSrc || document.querySelector('#thread-view video').src) === window.__ux.videoSourceBeforeRefresh), 'failed discussion refresh replaced the video source');
  assert((await page.locator('#discussion-comments').innerText()).includes(oldDiscussion.split('\n')[0]), 'failed discussion refresh removed the cached discussion');
  assert((await page.locator('#discussion-comments .discussion-refresh-status').innerText()).includes('still available'), 'refresh failure did not explain that cached comments remain available');
  await page.evaluate(() => {
    window.__ux.search.candidateCallsBeforeRefresh = window.__ux.search.streamCandidateCalls.filter(call => call.id === 't3_video').length;
    window.__ux.search.refreshError = false;
    window.__ux.search.refreshedMediaUrls = ['https://v.redd.it/updated/clip.mp4'];
  });
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.querySelector('.comments-header')?.textContent === '8 comments · 2 downloaded');
  await page.waitForFunction(() => window.__ux.search.streamCandidateCalls.filter(call => call.id === 't3_video').length > window.__ux.search.candidateCallsBeforeRefresh);
  assert((await page.locator('#thread-view').innerText()).includes('A newly downloaded reply.'), 'discussion refresh did not show new comments');
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.videoBeforeRefresh), 'successful discussion refresh remounted the video player');
  assert(await page.locator('.discussion-refresh').evaluate(node => node === document.activeElement), 'successful keyboard refresh did not restore focus to the refresh control');
  await page.locator('#tab-browse').click();

  // Keep the screenshots grounded in an actual populated subreddit feed.
  await page.setViewportSize({width:720, height:900});
  await openSettings();
  await page.locator('#theme-select').selectOption('classified');
  await page.evaluate(() => {
    const input = document.querySelector('#reader-text-scale');
    input.value = '125';
    input.dispatchEvent(new Event('input', {bubbles:true}));
  });
  await page.locator('#settings-close').click();
  await page.waitForFunction(() => document.querySelector('.header-tools > summary') === document.activeElement);
  assert(await page.locator('.header-tools > summary').evaluate(node => node === document.activeElement), 'closing Settings did not return focus to More tools');
  await page.locator('#cache-search-input').fill('same query');
  await page.waitForFunction(() => document.querySelector('#cache-search-list .cache-search-title')?.textContent === 'All Reddit downloaded result');
  const moreTools = page.locator('.header-tools > summary');
  assert(await moreTools.getAttribute('aria-label') === 'More tools', 'secondary tools disclosure has no accessible name');
  assert(!(await page.locator('.header-tools').evaluate(node => node.open)), 'secondary tools should start closed');
  const darkBox = await page.locator('#cache-search-results').boundingBox();
  assert(darkBox.x >= 0 && darkBox.x + darkBox.width <= 720, 'downloaded suggestions overflow at 720px');
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'search layout overflows at 720px');
  await page.screenshot({path:'artifacts/reader-ux/search-recovery-dark-720-closed.png'});
  await moreTools.click();
  assert(await page.locator('#settings-open').isVisible(), 'More tools does not expose Settings when opened');
  await page.screenshot({path:'artifacts/reader-ux/search-recovery-dark-720-tools-open.png'});
  await page.keyboard.press('Escape');
  assert(!(await page.locator('.header-tools').evaluate(node => node.open)), 'Escape did not close More tools');
  assert(await moreTools.evaluate(node => node === document.activeElement), 'Escape did not return focus to More tools');
  await moreTools.click();
  await page.locator('#subreddit-input').click();
  assert(!(await page.locator('.header-tools').evaluate(node => node.open)), 'outside interaction did not close More tools');
  assert(!(await page.locator('#cache-search-input').evaluate(node => node === document.activeElement)), 'outside interaction stole focus into search');
  await page.locator('.search-options > summary').click();
  assert(await page.locator('.search-options').evaluate(node => node.open), 'search options disclosure did not open');
  await page.keyboard.press('Escape');
  assert(!(await page.locator('.search-options').evaluate(node => node.open)), 'Escape did not close search options');
  assert(await page.locator('.search-options > summary').evaluate(node => node === document.activeElement), 'Escape did not return focus to search options');
  await page.locator('.search-options > summary').click();
  await page.screenshot({path:'artifacts/reader-ux/search-recovery-dark-720-options-open.png'});
  await page.locator('.search-options > summary').click();

  await page.setViewportSize({width:1100, height:900});
  await openSettings();
  await page.locator('#theme-select').selectOption('classified_light');
  await page.evaluate(() => {
    const input = document.querySelector('#reader-text-scale');
    input.value = '100';
    input.dispatchEvent(new Event('input', {bubbles:true}));
  });
  await page.locator('#settings-close').click();
  await page.locator('#cache-search-input').fill('same query');
  await page.waitForFunction(() => document.querySelector('#cache-search-list .cache-search-title')?.textContent === 'All Reddit downloaded result');
  const lightBox = await page.locator('#cache-search-results').boundingBox();
  assert(lightBox.x >= 0 && lightBox.x + lightBox.width <= 1100, 'downloaded suggestions overflow at 1100px');
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'search layout overflows at 1100px');
  await page.screenshot({path:'artifacts/reader-ux/search-recovery-light-1100.png'});
  await page.evaluate(() => { window.__ux.search.repeatCursor = true; window.__ux.search.holdPage = false; window.scrollTo(0, 0); });
  await waitForListing('soccer');
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25);
  await page.locator('#listing-load-more').click();
  await page.waitForFunction(() => document.querySelector('#listing-pagination-status').textContent.includes('same page again'));
  assert(!(await page.locator('#listing-load-more').isVisible()), 'repeated continuation cursor did not stop pagination');
  await page.evaluate(() => { window.__ux.search.repeatCursor = false; window.__ux.search.pageExpired = true; });
  await waitForListing('space');
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25);
  await page.locator('#listing-load-more').click();
  await page.waitForFunction(() => document.querySelector('#listing-pagination-status').textContent.includes('cursor expired'));
  assert(await page.locator('#listing-load-more').innerText() === 'Refresh listing', 'expired continuation did not offer a fresh listing action');
  await page.evaluate(() => {
    window.__ux.search.communityCountBeforeCursorRefresh = window.__ux.search.completedCommunities.length;
    window.__ux.search.freshCursorNext = true;
  });
  await page.locator('#listing-load-more').click();
  await page.waitForFunction(() => window.__ux.search.completedCommunities.length > window.__ux.search.communityCountBeforeCursorRefresh && !document.querySelector('#browse-submit').disabled);
  assert((await page.locator('#current-subreddit').innerText()).includes('Posts in r/space'), 'expired cursor recovery refreshed the wrong community');
  await page.waitForFunction(() => window.__ux.search.pageCalls.some(call => call.cursor === 'fresh-page-1'));
  const recoveredPaginationStatus = await page.locator('#listing-pagination-status').innerText();
  assert(!recoveredPaginationStatus.includes('cursor expired'), `fresh continuation reused an expired cursor; status: ${recoveredPaginationStatus}`);
  assert(await page.locator('.thread-item').count() >= 25, 'expired cursor recovery lost the clean first page');
  await page.evaluate(() => {
    Object.assign(window.__ux.search, {enableSearchPaging:true, searchPageError:true, holdSearchPage:true, searchPageCalls:[]});
  });
  await page.locator('#search-scope').selectOption('all');
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.locator('#cache-search-input').fill('continuation proof');
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25 && !document.querySelector('#header-search-submit').disabled);
  assert(await page.locator('#search-pagination').isVisible(), 'Reddit search did not expose continuation controls');
  if (!(await page.evaluate(() => window.__ux.search.searchPageCalls.length))) await page.locator('#search-load-more').click();
  await page.waitForFunction(() => window.__ux.search.searchPageCalls.length === 1 && typeof window.__ux.search.releaseSearchPage === 'function');
  assert(await page.evaluate(() => window.__ux.search.searchPageCalls.length === 1), 'initial search continuation triggered duplicate requests while becoming actionable');
  await page.evaluate(() => window.__ux.search.releaseSearchPage());
  await page.waitForFunction(() => document.querySelector('#search-pagination-status').textContent.includes('continuation unavailable'));
  const firstSearchPageIds = await page.evaluate(() => [...document.querySelectorAll('.thread-item')].map(node => node.dataset.threadId));
  assert(firstSearchPageIds.length === 25, 'failed search continuation removed or replaced current results');
  assert(await page.locator('#cache-search-input').inputValue() === 'continuation proof', 'failed search continuation cleared the query');
  await page.evaluate(() => { window.__ux.search.searchPageError = false; window.__ux.search.holdSearchPage = true; });
  await page.locator('#search-load-more').click();
  await page.waitForFunction(() => window.__ux.search.searchPageCalls.length >= 2 && typeof window.__ux.search.releaseSearchPage === 'function');
  assert(await page.locator('.thread-item').count() === 25, 'held search continuation changed visible results before it completed');
  const searchPageArgs = await page.evaluate(() => window.__ux.search.searchPageCalls);
  assert(searchPageArgs.length === 2, `retry issued ${searchPageArgs.length} continuation requests instead of one failed request and one retry`);
  assert(searchPageArgs[0].cursor === 'search-page-1' && searchPageArgs[1].cursor === searchPageArgs[0].cursor, 'search retry did not reuse the failed continuation cursor');
  assert(searchPageArgs[1].query === 'continuation proof' && searchPageArgs[1].subreddit === null && searchPageArgs[1].sort === 'relevance' && searchPageArgs[1].smart === true, 'search continuation lost its original query scope or options');
  const initialSearchArgs = await page.evaluate(() => window.__ux.calls.find(call => call.command === 'search_reddit' && call.args.query === 'continuation proof')?.args);
  assert(searchPageArgs[1].period === initialSearchArgs.period, 'search continuation lost the original time period');
  await page.evaluate(() => { window.__ux.search.firstSearchRow = document.querySelector('.thread-item'); window.__ux.search.searchPageScroll = window.scrollY; });
  await page.evaluate(() => window.__ux.search.releaseSearchPage());
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28 && document.querySelector('#search-pagination-status').textContent.includes('End of Reddit search') && document.activeElement.id === 'search-pagination-status');
  const pagedSearchIds = await page.evaluate(() => [...document.querySelectorAll('.thread-item')].map(node => node.dataset.threadId));
  assert(new Set(pagedSearchIds).size === 28 && firstSearchPageIds.every((id, index) => pagedSearchIds[index] === id), 'search continuation did not preserve source order and deduplicate overlapping results');
  assert(await page.evaluate(() => document.querySelector('.thread-item') === window.__ux.search.firstSearchRow), 'search continuation remounted existing result rows');
  assert(await page.evaluate(() => window.scrollY === window.__ux.search.searchPageScroll), 'search continuation moved the reader scroll position');
  await page.evaluate(() => Object.assign(window.__ux.search, {searchPageError:false, holdSearchPage:false, emptySearchTerminal:true, searchPageCalls:[]}));
  await page.locator('#cache-search-input').fill('empty terminal proof');
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25 && !document.querySelector('#header-search-submit').disabled);
  if (!(await page.evaluate(() => window.__ux.search.searchPageCalls.length))) await page.locator('#search-load-more').click();
  await page.waitForFunction(() => document.querySelector('#search-pagination-status').textContent.includes('End of Reddit search'));
  assert(await page.locator('.thread-item').count() === 25, 'empty terminal continuation removed the current search results');
  assert(await page.evaluate(() => window.__ux.search.searchPageCalls.some(call => call.cursor === 'search-page-1')), 'empty terminal continuation did not consume its cursor');

  await page.evaluate(() => {
    Object.assign(window.__ux.search, {error:false, enableSearchPaging:true, searchPageError:false, searchPageExpired:true, holdSearchPage:true, searchPageCalls:[]});
    window.scrollTo(0, 0);
  });
  await page.locator('#cache-search-input').fill('expired search proof');
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 25 && !document.querySelector('#header-search-submit').disabled);
  if (!(await page.evaluate(() => window.__ux.search.searchPageCalls.length))) await page.locator('#search-load-more').click();
  await page.waitForFunction(() => window.__ux.search.searchPageCalls.length === 1 && typeof window.__ux.search.releaseSearchPage === 'function');
  await page.evaluate(() => window.__ux.search.releaseSearchPage());
  await page.waitForFunction(() => document.querySelector('#search-pagination-status').textContent.includes('Search cursor expired'));
  assert(await page.locator('#search-load-more').innerText() === 'Search again', 'expired Reddit search cursor did not offer a fresh search action');
  await page.locator('#cache-search-input').fill('different search');
  assert(await page.locator('#search-pagination').isHidden(), 'expired-cursor recovery controls remained visible for a different query');
  await page.locator('#cache-search-input').fill('expired search proof');
  assert(await page.locator('#search-load-more').innerText() === 'Search again', 'restoring the original query did not restore its expired-cursor recovery action');
  const expiredSearchIds = await page.evaluate(() => {
    const ids = [...document.querySelectorAll('.thread-item')].map(node => node.dataset.threadId);
    window.__ux.search.expiredSearchIds = ids;
    return ids;
  });
  assert(expiredSearchIds.length === 25, 'expired search cursor removed the current Reddit results');

  await page.evaluate(() => { window.__ux.search.error = true; });
  await page.locator('#search-load-more').click();
  await page.waitForFunction(() => !document.querySelector('#header-search-submit').disabled && document.querySelector('#connection-status').textContent === 'Reddit search failed');
  assert(await page.evaluate(() => JSON.stringify([...document.querySelectorAll('.thread-item')].map(node => node.dataset.threadId)) === JSON.stringify(window.__ux.search.expiredSearchIds)), 'failed fresh search replaced the results from the expired listing');
  assert(await page.locator('.thread-item').count() === expiredSearchIds.length, 'failed fresh search removed some current results');
  assert(await page.locator('#cache-search-input').inputValue() === 'expired search proof', 'failed fresh search cleared its query');
  assert(await page.locator('#search-load-more').innerText() === 'Search again', 'failed fresh search removed the explicit recovery action');

  await page.evaluate(() => { Object.assign(window.__ux.search, {error:false, enableSearchPaging:false, searchPageExpired:false}); window.__ux.search.rows = [...window.__ux.search.rows].reverse(); });
  await page.locator('#search-load-more').click();
  await page.waitForFunction(() => document.querySelector('.thread-item')?.dataset.threadId === 't3_fixture27' && !document.querySelector('#header-search-submit').disabled);
  const freshSearchArgs = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_reddit' && call.args.query === 'expired search proof').map(call => call.args));
  const restartedArgs = freshSearchArgs.at(-1);
  assert(freshSearchArgs.length >= 3 && !Object.hasOwn(restartedArgs, 'cursor'), 'Search again reused the expired cursor instead of restarting the same search');
  assert(restartedArgs.query === 'expired search proof' && restartedArgs.subreddit === null && restartedArgs.sort === 'relevance' && restartedArgs.period === 'all' && restartedArgs.smart === true, 'Search again changed the original query scope or search options');
  assert(await page.locator('.thread-item').count() === 28, 'fresh search after cursor expiry did not replace results transactionally');
  return {passed:['failed subreddit preserves listing and focus','retry commits requested community','r/all redirect explanation and explicit search action','stale subreddit response cannot replace newer search','stale community completion preserves Library and clears busy state','downloaded search retry and retained focus/query','stale suggestion response cannot cross search scopes','pagination failure preserves posts and retries same cursor','pagination deduplicates and auto loads to exhaustion without scroll jump','continued media hint streams only on explicit play, survives offline save, and falls back locally','repeated continuation cursor stops safely','expired listing cursor offers a fresh transactional listing','Reddit search continuation preserves results, query scope, order, deduplication, retry cursor, scroll, and row identity','Reddit search treats an empty explicitly exhausted page as normal end-of-results','expired Reddit search cursor reruns the same search and preserves results on refresh failure','refresh rejects listing-only responses and preserves keyboard focus, comments, and video; refresh updates source total and new replies','More/search Options disclosures close with Escape and outside interaction','720px dark theme at enlarged text','1100px light theme']};
}
