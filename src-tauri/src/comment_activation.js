((postId, parentId, activationId, timeoutMs = 15000) => {
    window.__hrungnirCommentPageActivation = activationId;
    window.__hrungnirCommentPageActivationStatus = {
        id: activationId,
        status: "waiting",
    };
    if (!window.__hrungnirCommentPageControls) {
        window.__hrungnirCommentPageControls = Object.create(null);
    }

    const stateKey = `${postId}\u0000${parentId}`;
    const isCurrent = () => window.__hrungnirCommentPageActivation === activationId;
    const deadline = performance.now() + Math.max(0, Math.min(timeoutMs, 15000));
    const root = parentId === postId;

    const signature = (scope, button) => JSON.stringify([
        [...scope.querySelectorAll("shreddit-comment[thingid]")]
            .map(node => node.getAttribute("thingid")),
        [...scope.querySelectorAll("faceplate-partial[src]")]
            .map(node => node.getAttribute("src")),
        button.outerHTML,
    ]);

    const poll = () => {
        if (!isCurrent()) return;
        if (performance.now() >= deadline) {
            window.__hrungnirCommentPageActivationStatus = {
                id: activationId,
                status: "timed_out",
            };
            return;
        }
        // SSR controls can exist before Reddit's deferred component handlers
        // are ready. Waiting for load completion gives custom elements time to
        // register their click behavior before the single activation.
        if (document.readyState !== "complete") {
            setTimeout(poll, 100);
            return;
        }

        const scope = root
            ? document.querySelector("shreddit-comment-tree")
            : [...document.querySelectorAll("shreddit-comment[thingid]")]
                .find(node => node.getAttribute("thingid") === parentId);

        if (scope) {
            const visible = node => {
                const style = getComputedStyle(node);
                const box = node.getBoundingClientRect();
                return style.display !== "none"
                    && style.visibility !== "hidden"
                    && box.width > 0
                    && box.height > 0;
            };
            const enabled = node => !node.disabled
                && node.getAttribute("aria-disabled") !== "true"
                && !node.closest('[aria-busy="true"]');
            const button = [...scope.querySelectorAll("button")].find(node => {
                const label = (node.getAttribute("aria-label") || node.textContent || "").trim();
                const owner = node.closest("shreddit-comment[thingid]");
                const belongsToRequestedParent = root
                    ? !owner
                    : owner && owner.getAttribute("thingid") === parentId;
                return belongsToRequestedParent
                    && visible(node)
                    && enabled(node)
                    && (root ? /more comments|load more/i : /more replies/i).test(label);
            });

            if (button) {
                const previous = window.__hrungnirCommentPageControls[stateKey];
                const currentSignature = signature(scope, button);
                const previousStillCurrent = previous && previous.button.isConnected;
                if (!previousStillCurrent || previous.signature !== currentSignature) {
                    window.__hrungnirCommentPageControls[stateKey] = {
                        button,
                        signature: currentSignature,
                    };
                    const controlKeys = Object.keys(window.__hrungnirCommentPageControls);
                    if (controlKeys.length > 128) {
                        delete window.__hrungnirCommentPageControls[controlKeys[0]];
                    }
                    window.__hrungnirCommentPageActivationStatus = {
                        id: activationId,
                        status: "clicked",
                    };
                    button.click();
                    return;
                }
            }
        }
        setTimeout(poll, 100);
    };

    poll();
})
