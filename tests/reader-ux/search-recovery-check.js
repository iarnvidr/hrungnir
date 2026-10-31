async (page) => {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const waitForListing = async name => {
    await page.locator('#subreddit-input').fill(name);
    await page.locator('#browse-submit').click();
    await page.waitForFunction(() => !document.querySelector('#browse-submit').disabled);
  };

  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28);
  await waitForListing('soccer');
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
  await page.locator('#search-scope').selectOption('all');
  await page.waitForFunction(() => document.querySelector('#cache-search-list .cache-search-title')?.textContent === 'All Reddit downloaded result');
  assert(await page.locator('#search-scope').evaluate(node => node === document.activeElement), 'scope change lost focus while suggestions loaded');
  const cacheCalls = await page.evaluate(() => window.__ux.calls.filter(call => call.command === 'search_cache').slice(-2));
  assert(cacheCalls[0].args.subreddit === 'space' && cacheCalls[1].args.subreddit === null, 'cache search did not preserve its requested scope');
  await page.evaluate(() => window.__ux.search.releaseCache());
  await page.waitForFunction(() => window.__ux.search.completedCacheQueries.filter(item => item.query === 'same query').length === 2);
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 50)));
  assert(await page.locator('#cache-search-list .cache-search-title').innerText() === 'All Reddit downloaded result', 'late cache response replaced results from a newer scope');

  // Keep the screenshots grounded in an actual populated subreddit feed.
  await waitForListing('soccer');
  await page.setViewportSize({width:720, height:900});
  await page.locator('#settings-open').click();
  await page.locator('#theme-select').selectOption('classified');
  await page.evaluate(() => {
    const input = document.querySelector('#reader-text-scale');
    input.value = '125';
    input.dispatchEvent(new Event('input', {bubbles:true}));
  });
  await page.locator('#settings-close').click();
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
  await moreTools.click();
  assert(!(await page.locator('#settings-open').isVisible()), 'closing More tools leaves its menu exposed');
  await page.locator('.search-options > summary').click();
  assert(await page.locator('.search-options').evaluate(node => node.open), 'search options disclosure did not open');
  await page.screenshot({path:'artifacts/reader-ux/search-recovery-dark-720-options-open.png'});
  await page.locator('.search-options > summary').click();

  await page.setViewportSize({width:1100, height:900});
  await page.locator('#settings-open').click();
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
  return {passed:['failed subreddit preserves listing and focus','retry commits requested subreddit','r/all redirect explanation and explicit search action','stale subreddit response cannot replace newer search','stale community completion preserves Library and clears busy state','downloaded search retry and retained focus/query','stale suggestion response cannot cross search scopes','720px dark theme at enlarged text','1100px light theme']};
}
