async (page) => {
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  const assert = (value, message) => { if (!value) throw new Error(message); };
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28);
  await page.evaluate(() => {
    Object.assign(window.__ux.search, {
      provideMedia: true,
      commentPagesEnabled: true,
      commentPageError: true,
      holdCommentPage: false,
      commentPageCalls: [],
      comments: [{ id: 't1_parent', parent_id: 't3_video', body: 'A parent comment with more replies available.' }],
    });
  });
  await page.setViewportSize({ width: 720, height: 500 });
  await page.locator('[data-thread-id="t3_video"] .thread-open').click();
  await page.waitForFunction(() => window.__ux.posts.has('t3_video'));
  await page.evaluate(() => window.__ux.releasePost('t3_video'));
  await page.waitForFunction(() => document.querySelector('#discussion-comments .comments-header')?.textContent === '5 comments · 1 downloaded'
    && document.querySelector('[data-testid="comment-page-t1_parent"]')
    && document.querySelector('[data-testid="comment-page-t3_video"]'));
  await page.evaluate(() => {
    window.__ux.search.commentPlayer = document.querySelector('#thread-view video');
    window.scrollTo(0, document.body.scrollHeight);
  });
  assert(await page.locator('#thread-view video').count() === 1, 'fixture did not render the existing local video player');
  await page.locator('[data-testid="comment-page-t1_parent"]').click();
  await page.waitForFunction(() => document.querySelector('#discussion-comments .comment-page-status')?.textContent.includes('Comment continuation unavailable'));
  assert(await page.locator('#discussion-comments .comment').count() === 1, 'failed reply fetch removed the downloaded comment tree');
  assert((await page.locator('#discussion-comments .comments-header').innerText()).toLowerCase() === '5 comments · 1 downloaded', 'failed reply fetch confused source totals with downloaded comments');
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.search.commentPlayer), 'failed reply fetch replaced the current media player');
  await page.evaluate(() => { window.__ux.search.commentPageError = false; window.__ux.search.holdCommentPage = true; });
  await page.locator('[data-testid="comment-page-t1_parent"]').click();
  await page.waitForFunction(() => window.__ux.search.commentPageCalls.length === 2 && typeof window.__ux.search.releaseCommentPage === 'function');
  const preservedWhileLoading = await page.evaluate(() => ({
    comments: document.querySelectorAll('#discussion-comments .comment').length,
    body: document.querySelector('.comment[data-comment-id="t1_parent"] .comment-body')?.textContent,
    player: document.querySelector('#thread-view video') === window.__ux.search.commentPlayer,
    scroll: window.scrollY,
  }));
  await page.evaluate(() => { window.__ux.search.commentPageScroll = window.scrollY; });
  assert(preservedWhileLoading.comments === 1 && preservedWhileLoading.body.includes('more replies available'), 'held reply fetch changed existing discussion content');
  assert(preservedWhileLoading.player, 'held reply fetch replaced the current media player');
  await page.evaluate(() => { window.__ux.search.holdCommentPage = false; window.__ux.search.releaseCommentPage(); });
  await page.waitForFunction(() => document.querySelectorAll('#discussion-comments .comment').length === 3
    && document.querySelector('[data-testid="comment-page-t1_reply"]')
    && document.activeElement?.dataset?.commentId === 't1_parent');
  assert((await page.locator('#discussion-comments .comments-header').innerText()).toLowerCase() === '5 comments · 3 downloaded', 'reply page did not update the downloaded count separately');
  const calls = await page.evaluate(() => window.__ux.search.commentPageCalls);
  assert(calls[0].id === 't3_video' && calls[0].parent_id === 't1_parent' && calls[0].cursor === 'reply-1'
    && calls[1].cursor === calls[0].cursor, 'reply retry did not preserve its post, parent, and cursor scope');
  assert(await page.evaluate(() => window.scrollY === window.__ux.search.commentPageScroll), 'reply page moved the reader scroll position');
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.search.commentPlayer), 'reply page replaced the current media player');

  await page.locator('[data-testid="comment-page-t1_reply"]').click();
  await page.waitForFunction(() => document.querySelectorAll('#discussion-comments .comment').length === 4
    && document.querySelector('#discussion-comments .comments-header')?.textContent === '5 comments · 4 downloaded');
  const nestedCalls = await page.evaluate(() => window.__ux.search.commentPageCalls);
  assert(nestedCalls[2].parent_id === 't1_reply' && nestedCalls[2].cursor === 'nested-1', 'nested continuation was not scoped to its reply branch');
  assert(await page.locator('.partial-comments-note').isVisible(), 'partial discussion was presented as complete before all pages were exhausted');
  await page.locator('.comment[data-comment-id="t1_parent"] > .comment-card .comment-actions button').click();
  assert(await page.locator('.comment[data-comment-id="t1_parent"] > .children').isHidden(), 'reply branch did not collapse before top-level continuation');
  await page.locator('[data-testid="comment-page-t3_video"]').click();
  await page.waitForFunction(() => document.querySelectorAll('#discussion-comments .comment').length === 5
    && document.querySelector('#discussion-comments .comments-header')?.textContent === '5 comments · 5 downloaded'
    && !document.querySelector('.partial-comments-note'));
  assert(await page.evaluate(() => window.__ux.search.commentPageCalls[3]?.parent_id === 't3_video' && window.__ux.search.commentPageCalls[3]?.cursor === 'top-1'), 'top-level continuation was not scoped to the post’s remaining comments');
  assert(await page.locator('.comment[data-comment-id="t1_parent"] > .children').isHidden(), 'adding a top-level page expanded the collapsed reply branch');
  assert(await page.locator('#discussion-comments').getByText('All 5 comments reported by Reddit are downloaded.', {exact: true}).count() === 1, 'fully exhausted, source-count-matched discussion did not show its complete state');
  assert(await page.locator('.comment[data-comment-id="t1_parent"] > .comment-card .comment-body').innerText() === 'The refreshed parent comment.', 'overlapping source comment did not refresh its content');
  assert(await page.evaluate(() => [...document.querySelectorAll('.comment-list > .comment')].map(node => node.dataset.commentId).join(',') === 't1_parent,t1_second_root'), 'overlapping source comment update changed Reddit order');
  assert(await page.evaluate(() => document.querySelector('#thread-view video') === window.__ux.search.commentPlayer), 'comment completion replaced the current media player');

  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28);
  await page.evaluate(() => {
    Object.assign(window.__ux.search, {
      provideMedia: true,
      commentPagesEnabled: false,
      commentPageHandlesMissing: true,
      comments: [{ id: 't1_parent', parent_id: 't3_video', body: 'Cached reply branch without restart-safe cursor metadata.' }],
    });
  });
  await page.locator('[data-thread-id="t3_video"] .thread-open').click();
  await page.waitForFunction(() => window.__ux.posts.has('t3_video'));
  await page.evaluate(() => window.__ux.releasePost('t3_video'));
  await page.waitForFunction(() => document.querySelector('#discussion-comments .comments-header')?.textContent === '5 comments · 1 downloaded'
    && document.querySelector('.partial-comments-note'));
  assert(await page.locator('.comment[data-comment-id="t1_parent"] button[data-testid="collapse-t1_parent"]').count() === 0, 'cached partial discussion without memory-only handles was given a misleading no-replies control');
  assert(await page.locator('.discussion-refresh').count() === 1, 'cached partial discussion without continuation handles did not offer a way to reacquire them');
  return {passed:['reply continuation failure preserves comments, total count, and media','reply retry retains post/parent/cursor, focus, and scroll','nested reply pages append and preserve source total distinction','top-level continuation preserves collapsed branch and only reports complete after all cursors and source total agree','partial cached discussion without volatile cursors remains labeled as incomplete and can refresh']};
}
