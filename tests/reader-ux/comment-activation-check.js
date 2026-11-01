async (page) => {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const activationSource = await page.evaluate(() => window.parent.__nativeCommentActivationSource);
  assert(typeof activationSource === 'string' && activationSource.includes('shreddit-comment-tree'), 'runner did not provide the production comment activation helper source');
  await page.evaluate((source) => { window.__testCommentPageActivation = eval(source); }, activationSource);

  const reset = async (markup) => page.evaluate((html) => {
    document.querySelector('#comment-activation-fixture')?.remove();
    const host = document.createElement('div');
    host.id = 'comment-activation-fixture';
    host.style.cssText = 'position:fixed;z-index:2147483000;left:8px;top:8px;width:440px;max-height:240px;overflow:auto;background:Canvas;color:CanvasText;';
    const style = document.createElement('style');
    style.textContent = '#comment-activation-fixture shreddit-comment-tree,#comment-activation-fixture shreddit-comment,#comment-activation-fixture faceplate-partial{display:block;min-height:28px}#comment-activation-fixture button{display:inline-block;min-width:120px;min-height:28px}';
    host.append(style);
    const content = document.createElement('div');
    content.innerHTML = html;
    host.append(content);
    document.body.append(host);
    window.__commentActivationClicks = [];
    window.__hrungnirCommentPageControls = Object.create(null);
    window.__hrungnirCommentPageActivation = null;
    window.__hrungnirCommentPageActivationStatus = null;
    host.querySelectorAll('button[data-click-id]').forEach(button => button.addEventListener('click', () => window.__commentActivationClicks.push(button.dataset.clickId)));
  }, markup);
  const addButton = async (selector, clickId, label = 'More replies', attributes = {}) => page.evaluate(({selector, clickId, label, attributes}) => {
    const scope = document.querySelector(selector);
    if (!scope) throw new Error(`fixture scope not found: ${selector}`);
    const button = document.createElement('button');
    button.textContent = label;
    button.setAttribute('aria-label', label);
    button.dataset.clickId = clickId;
    for (const [name, value] of Object.entries(attributes)) {
      if (name === 'disabled') button.disabled = Boolean(value);
      else button.setAttribute(name, String(value));
    }
    button.addEventListener('click', () => window.__commentActivationClicks.push(clickId));
    scope.append(button);
    return true;
  }, {selector, clickId, label, attributes});
  const start = async (postId, parentId, activationId, timeout = 1000) => page.evaluate(({postId, parentId, activationId, timeout}) => {
    window.__testCommentPageActivation(postId, parentId, activationId, timeout);
  }, {postId, parentId, activationId, timeout});
  const sleepInPage = (milliseconds) => page.evaluate((delay) => new Promise(resolve => setTimeout(resolve, delay)), milliseconds);

  await reset('<shreddit-comment thingid="t1_parent"><shreddit-comment thingid="t1_child"><button data-click-id="nested">More replies</button></shreddit-comment></shreddit-comment>');
  await start('t3_post', 't1_parent', 'owner-delayed');
  await sleepInPage(180);
  assert(await page.evaluate(() => window.__commentActivationClicks.length === 0), 'nested child control was mistaken for the requested parent control');
  await addButton('shreddit-comment[thingid="t1_parent"]', 'parent');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'owner-delayed' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'parent'), 'delayed parent continuation did not activate exactly its own control');

  await reset('<shreddit-comment-tree><shreddit-comment thingid="t1_parent"><button data-click-id="nested-root">Load more comments</button></shreddit-comment></shreddit-comment-tree>');
  await start('t3_post', 't3_post', 'root-owner');
  await addButton('shreddit-comment-tree', 'tree', 'Load more comments');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'root-owner' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'tree'), 'thread-level continuation selected a nested comment button');

  await reset('<shreddit-comment thingid="t1_parent"><button disabled data-click-id="disabled">More replies</button><button aria-disabled="true" data-click-id="aria-disabled">More replies</button><div aria-busy="true"><button data-click-id="busy">More replies</button></div></shreddit-comment>');
  await start('t3_post', 't1_parent', 'replacement', 1400);
  await sleepInPage(180);
  assert(await page.evaluate(() => window.__commentActivationClicks.length === 0 && window.__hrungnirCommentPageActivationStatus.status === 'waiting'), 'disabled, aria-disabled, or busy continuation was activated');
  await addButton('shreddit-comment[thingid="t1_parent"]', 'replacement', 'More replies');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'replacement' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'replacement'), 'replacement continuation did not activate after disabled controls');

  await reset('<shreddit-comment thingid="t1_parent"><button data-click-id="stable">More replies</button></shreddit-comment>');
  await start('t3_post', 't1_parent', 'advance-one');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'advance-one' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  await start('t3_post', 't1_parent', 'same-page');
  await sleepInPage(180);
  assert(await page.evaluate(() => window.__hrungnirCommentPageActivationStatus?.id === 'same-page' && window.__hrungnirCommentPageActivationStatus.status === 'waiting' && window.__commentActivationClicks.length === 1), 'an unchanged prior control was clicked again without new page evidence');
  await page.evaluate(() => {
    const partial = document.createElement('faceplate-partial');
    partial.setAttribute('src', '/svc/shreddit/more-comments/r/example/post');
    document.querySelector('shreddit-comment[thingid="t1_parent"]').append(partial);
  });
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'same-page' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.length === 2), 'a changed continuation signature did not activate the next page');

  await reset('<shreddit-comment thingid="t1_parent"><button data-click-id="first-node">More replies</button></shreddit-comment>');
  await start('t3_post', 't1_parent', 'first-node');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'first-node' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  await page.evaluate(() => {
    const old = document.querySelector('shreddit-comment[thingid="t1_parent"] button');
    const replacement = document.createElement('button');
    replacement.dataset.clickId = 'first-node';
    replacement.textContent = 'More replies';
    replacement.addEventListener('click', () => window.__commentActivationClicks.push('replacement-node'));
    old.replaceWith(replacement);
  });
  await start('t3_post', 't1_parent', 'replacement-node');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'replacement-node' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'first-node,replacement-node'), 'a replaced continuation control with the same signature was not activated exactly once');

  await reset('<shreddit-comment thingid="t1_old"></shreddit-comment><shreddit-comment thingid="t1_new"></shreddit-comment>');
  await start('t3_post', 't1_old', 'superseded-old', 1200);
  await sleepInPage(120);
  await start('t3_post', 't1_new', 'superseding-new', 1200);
  await addButton('shreddit-comment[thingid="t1_old"]', 'old-late');
  await addButton('shreddit-comment[thingid="t1_new"]', 'new-current');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'superseding-new' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  await sleepInPage(140);
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'new-current'), 'superseded activation clicked a stale parent control');

  await reset('<shreddit-comment thingid="t1_missing"></shreddit-comment>');
  const timeoutStartedAt = await page.evaluate(() => performance.now());
  await start('t3_post', 't1_missing', 'bounded-timeout', 260);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'bounded-timeout' && window.__hrungnirCommentPageActivationStatus.status === 'timed_out');
  assert(await page.evaluate(() => window.__commentActivationClicks.length === 0), 'bounded timeout activated a missing continuation');
  assert(await page.evaluate(startedAt => performance.now() - startedAt < 500, timeoutStartedAt), 'missing continuation did not time out within the expected bound');

  return {passed:[
    'delayed reply control activates only its requested parent, not a nested child',
    'thread-level load-more control ignores nested comment buttons',
    'disabled, aria-disabled, and aria-busy controls wait for an enabled replacement',
    'same parent waits on an unchanged signature and advances after page signature changes',
    'same parent advances once when the prior button node is replaced with identical markup',
    'new activation supersedes pending work for an older parent',
    'missing continuation reports timeout within the requested bound',
  ]};
}
