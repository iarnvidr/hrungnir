async (page) => {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const activeId = () => page.evaluate(() => document.activeElement.closest('[data-thread-id]')?.dataset.threadId);
  await page.evaluate(() => localStorage.clear());
  await page.setViewportSize({width: 1200, height: 760});
  await page.reload();
  await page.waitForFunction(() => document.querySelectorAll('.thread-item').length === 28);
  for (const name of ['soccer', 'space']) {
    await page.locator('#subreddit-input').fill(name);
    await page.locator('#browse-submit').click();
    await page.waitForFunction(() => !document.querySelector('#browse-submit').disabled);
  }
  const search = page.locator('#cache-search-input');
  await page.keyboard.press('Meta+l');
  assert(await search.evaluate(node => node === document.activeElement), 'Cmd+L did not focus search');
  await page.keyboard.type('HJKL');
  assert(await search.inputValue() === 'HJKL', 'shift tab shortcuts intercepted text entry');
  await search.fill('gangs of london s02e08');
  await page.evaluate(() => { window.__ux.search.rows = window.__ux.threads.slice(0, 2); window.__ux.search.hold = true; });
  await page.locator('#header-search-submit').click();
  await page.waitForFunction(() => typeof window.__ux.search.release === 'function');
  // A native webview can blur the submitting control while a request is active.
  await page.evaluate(() => { document.activeElement.blur(); window.__ux.search.hold = false; window.__ux.search.release(); });
  await page.waitForFunction(() => document.activeElement.matches('.thread-open'));
  assert(await activeId() === 't3_article', 'mouse search did not focus the first result');
  await page.keyboard.press('ArrowDown');
  assert(await activeId() === 't3_video', 'down arrow did not navigate search results');
  await page.keyboard.press('ArrowUp');
  assert(await activeId() === 't3_article', 'up arrow did not navigate search results');
  await page.keyboard.press('ArrowRight');
  await page.waitForFunction(() => !document.querySelector('#reader-panel').hidden);
  await page.keyboard.press('ArrowLeft');
  assert(await page.locator('#browse-panel').isVisible(), 'left arrow did not return from the post');
  assert(await page.locator('#search-communities button').count() === 2, 'community refinement is missing');
  for (const button of await page.locator('.background-tab-open').all()) await button.click();
  await page.locator('.post-tab-open').first().click();
  await page.waitForFunction(() => !document.querySelector('#reader-panel').hidden);
  await page.keyboard.press('Shift+L');
  await page.waitForFunction(() => document.querySelectorAll('.post-tab-open')[1].getAttribute('aria-selected') === 'true');
  await page.keyboard.press('Shift+H');
  await page.waitForFunction(() => document.querySelectorAll('.post-tab-open')[0].getAttribute('aria-selected') === 'true');
  await page.keyboard.press('Shift+ArrowLeft');
  assert(await page.locator('#browse-panel').isVisible(), 'shift arrow did not switch to navigator');
  await page.keyboard.press('Shift+ArrowRight');
  assert(await page.locator('#reader-panel').isVisible(), 'shift arrow did not switch to post');
  assert(await page.locator('.workspace-tabs #tab-library').count() === 0, 'Library remains in the tab line');
  assert(await page.locator('#tab-library').isVisible(), 'Library is inaccessible');
  await page.evaluate(() => { document.activeElement.blur(); window.__ux.emit('tab-shortcut', 'focus-search'); });
  assert(await search.evaluate(node => node === document.activeElement), 'native search menu did not focus search');
  await page.keyboard.press('Escape');
  await page.evaluate(() => {
    const spacer = document.createElement('div');
    spacer.style.height = '2000px';
    document.querySelector('#reader-panel').append(spacer);
  });
  for (const post of ['horizontal', 'vertical']) for (const community of ['horizontal', 'vertical']) {
    await page.evaluate(() => window.scrollTo(0, 0));
    if (!(await page.locator('.header-tools').evaluate(node => node.open))) await page.locator('.header-tools > summary').click();
    await page.locator('#settings-open').click();
    await page.locator('#post-tabs-layout').selectOption(post);
    await page.locator('#subreddit-tabs-layout').selectOption(community);
    await page.locator('#settings-close').click();
    if (community === 'horizontal') {
      assert(!(await page.locator('.subreddit-tools').evaluate(node => node.open)), 'horizontal community toolbar should start compact');
      await page.locator('.subreddit-tools > summary').click();
      assert(await page.locator('#browse-form').isVisible(), 'horizontal community toolbar does not expose subreddit fetching');
      await page.locator('.subreddit-tools > summary').click();
    } else {
      assert(await page.locator('.subreddit-tools').evaluate(node => node.open), 'vertical community tools were hidden');
      assert(!(await page.locator('.subreddit-tools > summary').isVisible()), 'vertical community tools keep a redundant disclosure control');
    }
    assert(await page.locator('.subreddit-visit').first().isVisible(), `${post}/${community}: communities hidden in reader`);
    assert(await page.locator('.post-tab-open').first().isVisible(), `${post}/${community}: posts hidden in reader`);
    const posts = await page.locator('.post-tab-open').all();
    const p0 = await posts[0].boundingBox(), p1 = await posts[1].boundingBox();
    assert(post === 'vertical' ? p1.y > p0.y + 10 : Math.abs(p1.y - p0.y) < 2, `${post}/${community}: wrong post orientation`);
    const communities = await page.locator('.subreddit-visit').all();
    const c0 = await communities[0].boundingBox(), c1 = await communities[1].boundingBox();
    assert(community === 'vertical' ? c1.y > c0.y + 10 : Math.abs(c1.y - c0.y) < 2, `${post}/${community}: wrong community orientation`);
    for (const width of [1200, 1100, 720, 500]) {
      await page.setViewportSize({width, height: 760});
      const overflow = await page.evaluate(() => [...document.querySelectorAll('body *')].map(node => {
        const rect = node.getBoundingClientRect();
        return {tag:node.tagName, id:node.id, className:String(node.className || '').slice(0,60), left:Math.round(rect.left), right:Math.round(rect.right), width:Math.round(rect.width)};
      }).filter(item => item.width > 0 && (item.left < -1 || item.right > innerWidth + 1)).slice(0,6));
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), `${post}/${community}: horizontal page overflow at ${width}; elements=${JSON.stringify(overflow)}`);
    }
    await page.setViewportSize({width: 1200, height: 760});
    await page.evaluate(() => window.scrollTo(0, 500));
    for (const selector of ['.workspace-tabs', '.rail']) {
      const box = await page.locator(selector).boundingBox();
      assert(box.y >= -1 && box.y < 100, `${post}/${community}: ${selector} did not stay visible while scrolling`);
    }
  }
  await page.reload();
  await page.waitForFunction(() => document.querySelector('.post-tab-open'));
  assert(await page.evaluate(() => document.documentElement.dataset.postTabs === 'vertical' && document.documentElement.dataset.subredditTabs === 'vertical'), 'layout choices did not persist');
  await page.locator('.subreddit-visit').first().focus();
  const before = await page.locator('.subreddit-visit').first().getAttribute('data-subreddit');
  await page.keyboard.press('Shift+J');
  await page.waitForFunction(name => document.activeElement.matches('.subreddit-visit') && document.activeElement.dataset.subreddit !== name, before);
  await page.keyboard.press('Shift+K');
  await page.waitForFunction(name => document.activeElement.dataset.subreddit === name, before);
  return {passed: ['Cmd+L', 'typing safety', 'mouse search focus after blur', 'search arrows', 'community refinements', 'shift hjkl and arrows', 'Library toolbar', 'four simultaneous navigation layouts', 'narrow layouts', 'persisted orientations', 'stable community tab order']};
}
