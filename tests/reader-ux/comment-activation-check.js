async (page) => {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const nativeProbeSource = await page.evaluate(() => window.parent.__nativeUiProbeSource);
  assert(typeof nativeProbeSource === 'string' && nativeProbeSource.startsWith('/* Dedicated native test app only.'), 'runner did not provide the exact native UI probe source');
  const nativeProbeCompile = await page.evaluate((source) => {
    const compiled = new Function(source);
    return typeof compiled === 'function';
  }, nativeProbeSource);
  assert(nativeProbeCompile, 'native UI probe did not parse cleanly without executing its journey');

  const activationSource = await page.evaluate(() => window.parent.__nativeCommentActivationSource);
  assert(typeof activationSource === 'string' && activationSource.includes('faceplate-partial') && activationSource.includes('more-comments'), 'runner did not provide the production root-partial activation helper source');
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
    window.__commentActivationLegacyLoads = [];
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

  await reset('<shreddit-comment thingid="t1_parent"><shreddit-comment thingid="t1_nested"><button data-click-id="nested-singular">More reply</button></shreddit-comment><button data-click-id="parent-singular">More reply</button></shreddit-comment>');
  await start('t3_post', 't1_parent', 'singular-parent');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'singular-parent' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'parent-singular'), 'singular parent control was not selected after rejecting a nested singular control');

  await reset('<shreddit-comment-tree><shreddit-comment thingid="t1_parent"><button data-click-id="nested-root">Load more comments</button></shreddit-comment></shreddit-comment-tree>');
  await start('t3_post', 't3_post', 'root-owner', 260);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'root-owner' && window.__hrungnirCommentPageActivationStatus.status === 'timed_out');
  assert(await page.evaluate(() => window.__commentActivationClicks.length === 0), 'thread-level continuation activated a nested reply button instead of a root partial');

  await reset('<shreddit-comment-tree><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="wrong-post" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_other?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="unrelated-host" src="https://example.invalid/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial><shreddit-comment thingid="t1_nested-owner"><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="comment-owned" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?startingDepth=1&amp;commentParentPositions=4"></faceplate-partial></shreddit-comment><faceplate-partial data-lazy-id="requested-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    document.querySelectorAll('faceplate-partial[data-lazy-id]').forEach(partial => {
      partial.load = () => {
        window.__commentActivationLegacyLoads.push(partial.dataset.lazyId);
        return Promise.resolve();
      };
      partial.loadContent = () => {
        window.__commentActivationRootLoads.push(partial.dataset.lazyId);
        partial.dataset.loadResult = 'requested';
        return Promise.resolve();
      };
    });
  });
  await start('t3_post', 't3_post', 'root-public-load', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'root-public-load' && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial');
  assert(await page.evaluate(() => {
    const status = window.__hrungnirCommentPageActivationStatus;
    const partial = document.querySelector('faceplate-partial[data-lazy-id="requested-root"]');
    return status.diagnostics.activationMode === 'public_load_content'
      && status.diagnostics.rootPartialHasLoadContent === true
      && status.diagnostics.chosenMethod === 'loadContent'
      && status.diagnostics.publicLoadRejected === false
      && window.__commentActivationLegacyLoads.length === 0
      && window.__commentActivationRootLoads.join(',') === 'requested-root'
      && partial.dataset.loadResult === 'requested';
  }), 'public load activated anything other than the exact validated same-post root partial');

  await reset('<shreddit-comment-tree><faceplate-partial data-lazy-id="old-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    const old = document.querySelector('faceplate-partial[data-lazy-id="old-root"]');
    old.loadContent = () => {
      window.__commentActivationRootLoads.push('old');
      return Promise.resolve();
    };
  });
  await start('t3_post', 't3_post', 'old-root-first', 1200);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'old-root-first'
    && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial');
  await page.evaluate(() => {
    const next = document.createElement('faceplate-partial');
    next.dataset.lazyId = 'new-root';
    next.setAttribute('src', 'https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&rootCommentOffset=25&commentPositionOffset=0');
    next.loadContent = () => {
      window.__commentActivationRootLoads.push('new');
      return Promise.resolve();
    };
    document.querySelector('shreddit-comment-tree').append(next);
  });
  await start('t3_post', 't3_post', 'new-root-second', 1200);
  await page.waitForFunction(() => window.__commentActivationRootLoads.join(',') === 'old,new');
  assert(await page.evaluate(() => window.__hrungnirCommentPageActivationStatus?.id === 'new-root-second'
    && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial'), 'a fresh same-post root continuation was hidden behind an unchanged old partial');

  await reset('<shreddit-comment-tree><faceplate-partial data-lazy-id="rejected-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    const partial = document.querySelector('faceplate-partial[data-lazy-id="rejected-root"]');
    partial.load = () => {
      window.__commentActivationLegacyLoads.push(partial.dataset.lazyId);
      return Promise.resolve();
    };
    partial.loadContent = () => {
      window.__commentActivationRootLoads.push(partial.dataset.lazyId);
      return Promise.reject(new Error('fixture rejection'));
    };
  });
  await start('t3_post', 't3_post', 'root-public-load-rejected', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'root-public-load-rejected' && window.__hrungnirCommentPageActivationStatus.diagnostics.publicLoadRejected === true);
  assert(await page.evaluate(() => window.__commentActivationRootLoads.join(',') === 'rejected-root'
    && window.__commentActivationLegacyLoads.length === 0
    && window.__hrungnirCommentPageActivationStatus.diagnostics.activationMode === 'public_load_content'
    && window.__hrungnirCommentPageActivationStatus.diagnostics.publicLoadFailureName === 'Error'
    && window.__hrungnirCommentPageActivationStatus.diagnostics.publicLoadFailurePhase === 'async'), 'rejected public loadContent was not recorded without retrying or exposing an error event');

  await reset('<shreddit-comment-tree><faceplate-partial data-lazy-id="alert-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    const partial = document.querySelector('faceplate-partial[data-lazy-id="alert-root"]');
    partial.loadContent = () => {
      window.__commentActivationRootLoads.push('alert-root');
      if (window.__commentActivationRootLoads.length === 1) {
        partial.dispatchEvent(new CustomEvent('faceplate-alert', {
          detail: {cause: 'partial-request-network-error-503'},
        }));
      }
      return Promise.resolve();
    };
  });
  await start('t3_post', 't3_post', 'alert-first', 1000);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'alert-first'
    && window.__hrungnirCommentPageActivationStatus.diagnostics.publicLoadErrorObserved === true);
  await start('t3_post', 't3_post', 'alert-retry', 1000);
  await page.waitForFunction(() => window.__commentActivationRootLoads.length === 2);
  assert(await page.evaluate(() => window.__commentActivationRootLoads.length === 2
    && window.__hrungnirCommentPageActivationStatus?.id === 'alert-retry'
    && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial'), 'a target-owned fixed partial network alert did not permit one explicit retry');

  await reset('<shreddit-comment-tree><faceplate-partial data-lazy-id="descendant-alert-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"><span data-error-child></span></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    const partial = document.querySelector('faceplate-partial[data-lazy-id="descendant-alert-root"]');
    const child = partial.querySelector('[data-error-child]');
    partial.loadContent = () => {
      window.__commentActivationRootLoads.push('descendant-alert-root');
      child.dispatchEvent(new CustomEvent('faceplate-alert', {
        bubbles: true,
        detail: {cause: 'partial-request-network-error-503'},
      }));
      return Promise.resolve();
    };
  });
  await start('t3_post', 't3_post', 'descendant-alert-first', 1000);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'descendant-alert-first'
    && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial');
  await start('t3_post', 't3_post', 'descendant-alert-retry', 700);
  await sleepInPage(180);
  assert(await page.evaluate(() => window.__commentActivationRootLoads.length === 1
    && window.__hrungnirCommentPageActivationStatus?.id === 'descendant-alert-retry'
    && window.__hrungnirCommentPageActivationStatus.status === 'waiting_for_previous_target_change'
    && window.__hrungnirCommentPageActivationStatus.diagnostics.publicLoadErrorObserved === false), 'a bubbled descendant alert incorrectly unlocked a stale parent retry');

  await reset('<div style="height:900px"></div><shreddit-comment-tree><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="wrong-post" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_other?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="unrelated-host" src="https://example.invalid/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial><shreddit-comment thingid="t1_nested-owner"><faceplate-partial style="position:absolute;left:-2000px" data-lazy-id="comment-owned" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?startingDepth=1&amp;commentParentPositions=4"></faceplate-partial></shreddit-comment><faceplate-partial data-lazy-id="requested-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationIntersections = [];
    document.querySelectorAll('faceplate-partial[data-lazy-id]').forEach(partial => {
      const observer = new IntersectionObserver(entries => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            window.__commentActivationIntersections.push(partial.dataset.lazyId);
            observer.disconnect();
          }
        }
      });
      observer.observe(partial);
    });
  });
  const rootPartialWasOffscreen = await page.evaluate(() => {
    const partial = document.querySelector('faceplate-partial[data-lazy-id="requested-root"]');
    return partial.getBoundingClientRect().top > innerHeight;
  });
  assert(rootPartialWasOffscreen, 'root continuation fixture was not initially below the viewport');
  await start('t3_post', 't3_post', 'root-partial', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'root-partial' && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial');
  await page.waitForFunction(() => window.__commentActivationIntersections?.includes('requested-root'));
  assert(await page.evaluate(() => {
    const status = window.__hrungnirCommentPageActivationStatus;
    const partial = document.querySelector('faceplate-partial[data-lazy-id="requested-root"]');
    return status.diagnostics.activationMode === 'scroll_fallback'
      && window.__commentActivationIntersections.join(',') === 'requested-root'
      && partial.getBoundingClientRect().top < innerHeight;
  }), 'root activation did not scroll the same-post top-level lazy partial into the real IntersectionObserver viewport');

  await reset('<div style="height:900px"></div><shreddit-comment thingid="t1_parent"><button data-click-id="offscreen-reply">More replies</button></shreddit-comment>');
  const replyWasOffscreen = await page.evaluate(() => document.querySelector('shreddit-comment[thingid="t1_parent"] button').getBoundingClientRect().top > innerHeight);
  assert(replyWasOffscreen, 'nested continuation fixture was not initially below the viewport');
  await start('t3_post', 't1_parent', 'offscreen-reply', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'offscreen-reply' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'offscreen-reply'
    && document.querySelector('shreddit-comment[thingid="t1_parent"] button').getBoundingClientRect().top < innerHeight), 'nested activation did not scroll the requested offscreen reply control into view exactly once');

  await reset('<shreddit-comment thingid="t1_parent"><faceplate-partial data-lazy-id="wrong-query" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?unknown=1"></faceplate-partial><shreddit-comment thingid="t1_child"><faceplate-partial data-lazy-id="descendant" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?startingDepth=2&amp;commentParentPositions=4,0"></faceplate-partial></shreddit-comment><faceplate-partial data-lazy-id="requested-nested" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?startingDepth=1&amp;commentParentPositions=5"></faceplate-partial></shreddit-comment>');
  await page.evaluate(() => {
    window.__commentActivationPartialLoads = [];
    document.querySelectorAll('faceplate-partial[data-lazy-id]').forEach(partial => {
      partial.loadContent = () => {
        window.__commentActivationPartialLoads.push(partial.dataset.lazyId);
        return Promise.resolve();
      };
    });
  });
  await start('t3_post', 't1_parent', 'nested-public-load', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'nested-public-load' && window.__hrungnirCommentPageActivationStatus.status === 'activated_nested_partial');
  assert(await page.evaluate(() => {
    const status = window.__hrungnirCommentPageActivationStatus;
    return status.diagnostics.activationMode === 'public_load_content'
      && status.diagnostics.parentOwnedPartialCount === 1
      && status.diagnostics.parentOwnedLoadContentCount === 1
      && window.__commentActivationPartialLoads.join(',') === 'requested-nested';
  }), 'nested request did not activate exactly its own validated public partial');

  await reset('<shreddit-comment-tree><faceplate-partial data-load-id="busy-root" src="https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&amp;comments-remaining=25&amp;commentPositionOffset=0"><span>initial placeholder</span></faceplate-partial></shreddit-comment-tree>');
  await page.evaluate(() => {
    window.__commentActivationRootLoads = [];
    const current = document.querySelector('faceplate-partial[data-load-id="busy-root"]');
    current.isLoading = false;
    current.loadContent = () => {
      window.__commentActivationRootLoads.push('first');
      current.isLoading = true;
      current.innerHTML = '<span>response placeholder</span>';
      const unrelated = document.createElement('shreddit-comment');
      unrelated.setAttribute('thingid', 't1_unrelated');
      document.querySelector('shreddit-comment-tree').append(unrelated);
      return new Promise(resolve => {
        window.__commitCommentFragment = () => {
          const next = document.createElement('faceplate-partial');
          next.dataset.loadId = 'next-root';
          next.setAttribute('src', 'https://www.reddit.com/svc/shreddit/more-comments/testsub/t3_post?top-level=1&rootCommentOffset=25&commentPositionOffset=0');
          next.loadContent = () => {
            window.__commentActivationRootLoads.push('next');
            return Promise.resolve();
          };
          current.replaceWith(next);
          resolve();
        };
      });
    };
  });
  await start('t3_post', 't3_post', 'partial-body-first', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'partial-body-first' && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial');
  await start('t3_post', 't3_post', 'partial-body-second', 1800);
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'partial-body-second'
    && window.__hrungnirCommentPageActivationStatus.diagnostics.targetIsLoading === true);
  // Release headers separately from the body/render commit, as the source does.
  await page.evaluate(() => { document.querySelector('faceplate-partial[data-load-id="busy-root"]').isLoading = false; });
  await sleepInPage(140);
  assert(await page.evaluate(() => window.__commentActivationRootLoads.join(',') === 'first'
    && window.__hrungnirCommentPageActivationStatus?.id === 'partial-body-second'
    && window.__hrungnirCommentPageActivationStatus.status === 'waiting_for_previous_target_change'), 'same source control was called again while only loading/placeholder/unrelated DOM state changed');
  await page.evaluate(() => window.__commitCommentFragment());
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'partial-body-second'
    && window.__hrungnirCommentPageActivationStatus.status === 'activated_root_partial'
    && window.__commentActivationRootLoads.join(',') === 'first,next');

  await reset('<shreddit-comment thingid="t1_parent"><button disabled data-click-id="disabled">More replies</button><button aria-disabled="true" data-click-id="aria-disabled">More replies</button><div aria-busy="true"><button data-click-id="busy">More replies</button></div></shreddit-comment>');
  await start('t3_post', 't1_parent', 'replacement', 1400);
  await sleepInPage(180);
  const disabledControlsState = await page.evaluate(() => ({
    clicks: window.__commentActivationClicks,
    status: window.__hrungnirCommentPageActivationStatus,
    controls: [...document.querySelectorAll('#comment-activation-fixture button')].map(button => ({
      id: button.dataset.clickId,
      disabled: button.disabled,
      ariaDisabled: button.getAttribute('aria-disabled'),
      busyAncestor: Boolean(button.closest('[aria-busy="true"]')),
    })),
  }));
  assert(disabledControlsState.clicks.length === 0 && disabledControlsState.status.status === 'waiting_for_reply_button', `disabled, aria-disabled, or busy continuation was activated: ${JSON.stringify(disabledControlsState)}`);
  await addButton('shreddit-comment[thingid="t1_parent"]', 'replacement', 'More replies');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'replacement' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  assert(await page.evaluate(() => window.__commentActivationClicks.join(',') === 'replacement'), 'replacement continuation did not activate after disabled controls');

  await reset('<shreddit-comment thingid="t1_parent"><button data-click-id="stable">More replies</button></shreddit-comment>');
  await start('t3_post', 't1_parent', 'advance-one');
  await page.waitForFunction(() => window.__hrungnirCommentPageActivationStatus?.id === 'advance-one' && window.__hrungnirCommentPageActivationStatus.status === 'clicked');
  await start('t3_post', 't1_parent', 'same-page');
  await sleepInPage(180);
  assert(await page.evaluate(() => window.__hrungnirCommentPageActivationStatus?.id === 'same-page' && window.__hrungnirCommentPageActivationStatus.status === 'waiting_for_previous_target_change' && window.__commentActivationClicks.length === 1), 'an unchanged prior control was clicked again without new page evidence');
  await page.evaluate(() => {
    const button = document.querySelector('shreddit-comment[thingid="t1_parent"] button');
    button.textContent = 'More replies, next page';
    button.setAttribute('aria-label', 'More replies, next page');
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
    'native UI probe source parses in the browser without running its native journey',
    'delayed reply control activates only its requested parent, not a nested child',
    'singular reply control activates only its requested parent, not a nested child',
    'thread-level load-more control ignores nested comment buttons',
    'public load activates exactly one validated same-post root partial',
    'fresh same-post root source wins over a retained old partial',
    'nested requests activate a validated partial owned by the exact parent and ignore invalid or descendant partials',
    'partial activation waits for source identity to change after isLoading clears and placeholder DOM changes',
    'target-owned fixed partial network alerts permit an explicit retry without exposing alert details',
    'bubbled descendant partial alerts cannot unlock a stale parent retry',
    'root activation scrolls only the same-post top-level lazy partial into IntersectionObserver view',
    'nested reply activation scrolls its own offscreen parent control into view',
    'disabled, aria-disabled, and aria-busy controls wait for an enabled replacement',
    'same parent waits on an unchanged signature and advances after page signature changes',
    'same parent advances once when the prior button node is replaced with identical markup',
    'new activation supersedes pending work for an older parent',
    'missing continuation reports timeout within the requested bound',
  ]};
}
