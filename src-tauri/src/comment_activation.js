((postId, parentId, activationId, timeoutMs = 15000) => {
    window.__hrungnirCommentPageActivation = activationId;
    const diagnostics = {};
    const publish = status => {
        window.__hrungnirCommentPageActivationStatus = {
            id: activationId,
            status,
            diagnostics: { ...diagnostics },
        };
    };
    publish("waiting");
    if (!window.__hrungnirCommentPageControls) {
        window.__hrungnirCommentPageControls = Object.create(null);
    }

    const stateKey = `${postId}\u0000${parentId}`;
    const isCurrent = () => window.__hrungnirCommentPageActivation === activationId;
    const deadline = performance.now() + Math.max(0, Math.min(timeoutMs, 15000));
    const root = parentId === postId;
    const publicLoadErrorName = error => {
        const name = error && typeof error.name === "string" ? error.name : "";
        return ["TypeError", "NetworkError", "SecurityError", "AbortError", "Error"].includes(name)
            ? name
            : "other";
    };

    const signature = (scope, target) => JSON.stringify([
        [...scope.querySelectorAll("shreddit-comment[thingid]")]
            .map(node => node.getAttribute("thingid")),
        [...scope.querySelectorAll("faceplate-partial[src]")]
            .map(node => node.getAttribute("src")),
        target.outerHTML,
    ]);

    const poll = () => {
        if (!isCurrent()) return;
        if (performance.now() >= deadline) {
            diagnostics.readyState = document.readyState;
            publish("timed_out");
            return;
        }
        diagnostics.readyState = document.readyState;
        // SSR controls can exist before Reddit's deferred component handlers
        // are ready. Waiting for load completion gives custom elements time to
        // register their click behavior before the single activation.
        if (document.readyState !== "complete") {
            setTimeout(poll, 100);
            return;
        }

        const scope = root
            ? document
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
            const rootPartialMatches = node => {
                if (node.closest("shreddit-comment[thingid]")) return false;
                try {
                    const url = new URL(node.getAttribute("src"), location.href);
                    const parts = url.pathname.split("/").filter(Boolean);
                    return url.protocol === "https:"
                        && ["www.reddit.com", "reddit.com", "sh.reddit.com", "old.reddit.com"]
                            .includes(url.hostname)
                        && parts.length === 5
                        && parts[0] === "svc"
                        && parts[1] === "shreddit"
                        && parts[2] === "more-comments"
                        && /^[A-Za-z0-9_]{2,21}$/.test(parts[3])
                        && parts[4] === postId;
                } catch (_) {
                    return false;
                }
            };
            const rootPartials = root
                ? [...document.querySelectorAll("faceplate-partial[src]")]
                : [];
            const matchingRootPartials = rootPartials.filter(rootPartialMatches);
            diagnostics.rootPartialCount = rootPartials.length;
            diagnostics.matchingRootPartialCount = matchingRootPartials.length;
            diagnostics.requestedCommentScopeFound = Boolean(scope);
            const button = root ? null : [...scope.querySelectorAll("button")].find(node => {
                const label = (node.getAttribute("aria-label") || node.textContent || "").trim();
                const owner = node.closest("shreddit-comment[thingid]");
                return owner && owner.getAttribute("thingid") === parentId
                    && visible(node)
                    && enabled(node)
                    && /\bmore repl(?:y|ies)\b/i.test(label);
            });

            // Reddit's top-level continuation is rendered as a lazy
            // faceplate-partial outside the comment tree, not a button. Its
            // own IntersectionObserver starts the public request once it is
            // brought into view. Restrict it to the captured post and to a
            // root-level element; never construct or fetch its URL here.
            const rootPartial = matchingRootPartials[0] || null;
            const target = root ? rootPartial : button;
            diagnostics.parentOwnedReplyButtonFound = Boolean(button);
            if (rootPartial) {
                const loading = (rootPartial.getAttribute("loading") || "").toLowerCase();
                diagnostics.rootPartialLoading = ["lazy", "eager", "action", "programmatic", "auto"]
                    .includes(loading) ? loading : "other";
                diagnostics.pageVisibility = document.prerendering
                    ? "prerender"
                    : document.visibilityState === "visible" ? "visible" : "hidden";
                diagnostics.rootPartialDefined = typeof customElements !== "undefined"
                    && Boolean(customElements.get("faceplate-partial"));
                diagnostics.rootPartialHasLoadContent = typeof rootPartial.loadContent === "function";
            }

            const targetCanActivate = root
                ? target && target.isConnected && (() => {
                    const style = getComputedStyle(target);
                    return style.display !== "none" && style.visibility !== "hidden";
                })()
                : target && visible(target);

            if (targetCanActivate) {
                const previous = window.__hrungnirCommentPageControls[stateKey];
                const currentSignature = signature(scope, target);
                const previousStillCurrent = previous && previous.button.isConnected;
                diagnostics.previousTargetStillConnected = Boolean(previousStillCurrent);
                diagnostics.previousTargetChanged = Boolean(!previousStillCurrent
                    || previous.signature !== currentSignature);
                const box = target.getBoundingClientRect();
                diagnostics.targetBox = {
                    width: Math.round(box.width),
                    height: Math.round(box.height),
                    inViewport: box.bottom > 0 && box.top < window.innerHeight,
                };
                if (!previousStillCurrent || previous.signature !== currentSignature) {
                    window.__hrungnirCommentPageControls[stateKey] = {
                        button: target,
                        signature: currentSignature,
                    };
                    const controlKeys = Object.keys(window.__hrungnirCommentPageControls);
                    if (controlKeys.length > 128) {
                        delete window.__hrungnirCommentPageControls[controlKeys[0]];
                    }
                    const publicLoad = root && typeof target.loadContent === "function";
                    diagnostics.activationMode = root
                        ? publicLoad ? "public_load_content" : "scroll_fallback"
                        : "reply_button_click";
                    diagnostics.chosenMethod = root ? publicLoad ? "loadContent" : "scroll" : "click";
                    if (publicLoad) diagnostics.publicLoadRejected = false;
                    publish(root ? "activated_root_partial" : "clicked");
                    if (root) {
                        if (publicLoad) {
                            try {
                                const loading = target.loadContent();
                                if (loading && typeof loading.then === "function") {
                                    Promise.resolve(loading).catch(error => {
                                        if (!isCurrent()) return;
                                        diagnostics.publicLoadRejected = true;
                                        diagnostics.publicLoadFailureName = publicLoadErrorName(error);
                                        diagnostics.publicLoadFailurePhase = "async";
                                        publish("activated_root_partial");
                                    });
                                }
                            } catch (_) {
                                diagnostics.publicLoadRejected = true;
                                diagnostics.publicLoadFailureName = publicLoadErrorName(_);
                                diagnostics.publicLoadFailurePhase = "sync";
                                publish("activated_root_partial");
                            }
                        } else {
                            target.scrollIntoView({ block: "center", inline: "nearest" });
                        }
                    } else {
                        target.scrollIntoView({ block: "center", inline: "nearest" });
                        target.click();
                    }
                    return;
                }
                publish("waiting_for_previous_target_change");
            } else {
                publish(root ? "waiting_for_root_partial" : "waiting_for_reply_button");
            }
        } else {
            diagnostics.requestedCommentScopeFound = false;
            diagnostics.rootPartialCount = 0;
            diagnostics.matchingRootPartialCount = 0;
            diagnostics.parentOwnedReplyButtonFound = false;
            publish("waiting_for_requested_comment_scope");
        }
        setTimeout(poll, 100);
    };

    poll();
})
