((postId, parentId, activationId, timeoutMs = 15000) => {
    window.__hrungnirCommentPageActivation = activationId;
    const diagnostics = {};
    diagnostics.parentOwnedPartialCount = 0;
    diagnostics.parentOwnedLoadContentCount = 0;
    diagnostics.targetIsLoading = false;
    diagnostics.publicLoadErrorObserved = false;
    diagnostics.publicLoadRejected = false;
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

    const commentSourceSignature = target => JSON.stringify(["partial",
        target.getAttribute("src"),
        target.getAttribute("method"),
        [...target.querySelectorAll('input[type="hidden"]')]
            .filter(input => input.closest("faceplate-partial[src]") === target)
            .map(input => [
            input.getAttribute("name"),
            input.value,
            ]),
    ]);

    const buttonSignature = target => JSON.stringify(["button",
        target.getAttribute("aria-label"),
        target.textContent,
        target.disabled,
        target.getAttribute("aria-disabled"),
        [...(target.closest("form")?.querySelectorAll('input[type="hidden"]') || [])]
            .filter(input => input.closest("form") === target.closest("form"))
            .map(input => [input.getAttribute("name"), input.value]),
    ]);

    const partialMatches = (node, owner) => {
        if (owner) {
            if (node.closest("shreddit-comment[thingid]") !== owner) return false;
        } else if (node.closest("shreddit-comment[thingid]")) {
            return false;
        }
        try {
            const url = new URL(node.getAttribute("src"), location.href);
            const parts = url.pathname.split("/").filter(Boolean);
            const postSuffix = postId.startsWith("t3_") ? postId.slice(3) : "";
            if (url.protocol !== "https:"
                || !["www.reddit.com", "reddit.com", "sh.reddit.com", "old.reddit.com"].includes(url.hostname)
                || url.username || url.password || url.port || url.hash
                || url.pathname.endsWith("/") || url.pathname.includes("//")
                || parts.length !== 5
                || parts[0] !== "svc" || parts[1] !== "shreddit" || parts[2] !== "more-comments"
                || !/^[A-Za-z0-9_]{2,21}$/.test(parts[3])
                || parts[4] !== postId
                || !/^[A-Za-z0-9]{3,16}$/.test(postSuffix)
                || url.search.length > 2048) return false;

            const entries = [...url.searchParams.entries()];
            if (!entries.length || entries.length > 8) return false;
            const fields = new Map();
            for (const [key, value] of entries) {
                if (value.length > 512 || /[\u0000-\u001f\u007f-\u009f]/.test(value) || fields.has(key)) return false;
                fields.set(key, value);
            }
            const bounded = value => /^\d+$/.test(value) && Number(value) <= 10000;
            if ([...fields.keys()].every(key => ["startingDepth", "commentParentPositions", "commentPositionOffset", "sort", "seeker-session"].includes(key))
                && fields.has("startingDepth") && fields.has("commentParentPositions")) {
                const depth = fields.get("startingDepth");
                const positions = fields.get("commentParentPositions").split(",");
                if (!bounded(depth) || Number(depth) < 1 || Number(depth) > 128
                    || positions.length !== Number(depth) || positions.length > 128
                    || positions.some(position => !bounded(position))
                    || (fields.has("commentPositionOffset") && !bounded(fields.get("commentPositionOffset")))
                    || fields.has("sort") !== fields.has("seeker-session")) return false;
                return !fields.has("sort")
                    || (fields.get("sort") === "CONFIDENCE" && fields.get("seeker-session") === "true");
            }
            const topLevel = fields.get("top-level") === "1";
            const offset = fields.get("commentPositionOffset");
            const legacyRootOffset = fields.get("rootCommentOffset");
            const remaining = fields.get("comments-remaining");
            const knownLegacy = [...fields.keys()].every(key => ["top-level", "commentPositionOffset", "rootCommentOffset", "comments-remaining"].includes(key));
            return knownLegacy && fields.size === 3 && topLevel && bounded(offset)
                && ((legacyRootOffset !== undefined && bounded(legacyRootOffset) && remaining === undefined)
                    || (remaining !== undefined && bounded(remaining) && legacyRootOffset === undefined));
        } catch (_) {
            return false;
        }
    };

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
            const previous = window.__hrungnirCommentPageControls[stateKey];
            const preferChangedSource = candidates => {
                const changed = candidates.find(node => commentSourceSignature(node) !== previous?.signature);
                return changed || candidates.find(node => node === previous?.target) || candidates[0] || null;
            };
            const rootPartials = root
                ? [...document.querySelectorAll("faceplate-partial[src]")]
                : [];
            const matchingRootPartials = rootPartials.filter(node => partialMatches(node, null));
            const matchingParentPartials = !root
                ? [...scope.querySelectorAll("faceplate-partial[src]")]
                    .filter(node => partialMatches(node, scope))
                : [];
            diagnostics.rootPartialCount = rootPartials.length;
            diagnostics.matchingRootPartialCount = matchingRootPartials.length;
            diagnostics.parentOwnedPartialCount = matchingParentPartials.length;
            diagnostics.parentOwnedLoadContentCount = matchingParentPartials.filter(node => typeof node.loadContent === "function").length;
            diagnostics.requestedCommentScopeFound = Boolean(scope);
            const button = root || matchingParentPartials.length ? null : [...scope.querySelectorAll("button")].find(node => {
                const label = (node.getAttribute("aria-label") || node.textContent || "").trim();
                const owner = node.closest("shreddit-comment[thingid]");
                return owner && owner.getAttribute("thingid") === parentId
                    && visible(node)
                    && enabled(node)
                    && /\bmore repl(?:y|ies)\b/i.test(label);
            });

            // Continuations belong either to the root or to one exact comment.
            // Use the source element's public loader without constructing a
            // request. Capturing its response can finish before its DOM render,
            // so unchanged source identity must wait for the actual next control.
            const rootPartial = preferChangedSource(matchingRootPartials);
            const parentPartial = preferChangedSource(matchingParentPartials);
            const partial = root ? rootPartial : parentPartial;
            const target = partial || button;
            diagnostics.parentOwnedReplyButtonFound = Boolean(button);
            if (partial) {
                const loading = (partial.getAttribute("loading") || "").toLowerCase();
                diagnostics.rootPartialLoading = ["lazy", "eager", "action", "programmatic", "auto"]
                    .includes(loading) ? loading : "other";
                diagnostics.pageVisibility = document.prerendering
                    ? "prerender"
                    : document.visibilityState === "visible" ? "visible" : "hidden";
                diagnostics.rootPartialDefined = typeof customElements !== "undefined"
                    && Boolean(customElements.get("faceplate-partial"));
                diagnostics.rootPartialHasLoadContent = typeof partial.loadContent === "function";
            }

            const targetCanActivate = partial
                ? target && target.isConnected && (() => {
                    const style = getComputedStyle(target);
                    return style.display !== "none" && style.visibility !== "hidden";
                })()
                : target && visible(target);

            if (targetCanActivate) {
                const sourceControl = Boolean(partial);
                const currentSignature = sourceControl
                    ? commentSourceSignature(target)
                    : buttonSignature(target);
                const previousStillCurrent = previous && previous.target.isConnected;
                const targetIsLoading = sourceControl && target.isLoading === true;
                const sameTarget = previous && previous.target === target;
                diagnostics.previousTargetStillConnected = Boolean(previousStillCurrent);
                diagnostics.targetIsLoading = Boolean(targetIsLoading);
                diagnostics.previousTargetChanged = Boolean(!previous || !sameTarget || previous.signature !== currentSignature);
                const box = target.getBoundingClientRect();
                diagnostics.targetBox = {
                    width: Math.round(box.width),
                    height: Math.round(box.height),
                    inViewport: box.bottom > 0 && box.top < window.innerHeight,
                };
                const sourceUnchanged = previous && previous.signature === currentSignature;
                const previousLoadFailed = previous && previous.publicLoadErrorObserved === true;
                const unchangedTarget = sourceUnchanged && (sourceControl || sameTarget);
                if (targetIsLoading || (unchangedTarget && !previousLoadFailed)) {
                    publish("waiting_for_previous_target_change");
                    setTimeout(poll, 100);
                    return;
                }
                if (!previous || !unchangedTarget || previousLoadFailed) {
                    if (typeof previous?.cleanup === "function") previous.cleanup();
                    const controlState = {
                        target,
                        signature: currentSignature,
                    };
                    window.__hrungnirCommentPageControls[stateKey] = controlState;
                    const controlKeys = Object.keys(window.__hrungnirCommentPageControls);
                    if (controlKeys.length > 128) {
                        const evicted = window.__hrungnirCommentPageControls[controlKeys[0]];
                        if (typeof evicted?.cleanup === "function") evicted.cleanup();
                        delete window.__hrungnirCommentPageControls[controlKeys[0]];
                    }
                    const publicLoad = sourceControl && typeof target.loadContent === "function";
                    diagnostics.activationMode = sourceControl
                        ? publicLoad ? "public_load_content" : "scroll_fallback"
                        : "reply_button_click";
                    diagnostics.chosenMethod = sourceControl ? publicLoad ? "loadContent" : "scroll" : "click";
                    if (publicLoad) diagnostics.publicLoadRejected = false;
                    publish(sourceControl
                        ? root ? "activated_root_partial" : "activated_nested_partial"
                        : "clicked");
                    if (sourceControl) {
                        if (publicLoad) {
                            const activationStatus = root ? "activated_root_partial" : "activated_nested_partial";
                            const onLoadError = event => {
                                if (event.target !== target) return;
                                controlState.publicLoadErrorObserved = true;
                                if (!isCurrent()) return;
                                diagnostics.publicLoadErrorObserved = true;
                                diagnostics.publicLoadFailureName = publicLoadErrorName(event.error || event.detail);
                                publish(activationStatus);
                            };
                            const onLoadAlert = event => {
                                if (event.target !== target) return;
                                const cause = event.detail && event.detail.cause;
                                if (cause !== "partial-request-fetch-failure"
                                    && cause !== "partial-request-uncaught-failure"
                                    && !(typeof cause === "string" && /^partial-request-network-error-[45]\d\d$/.test(cause))) return;
                                controlState.publicLoadErrorObserved = true;
                                if (!isCurrent()) return;
                                diagnostics.publicLoadErrorObserved = true;
                                publish(activationStatus);
                            };
                            target.addEventListener("faceplate-error", onLoadError);
                            target.addEventListener("faceplate-alert", onLoadAlert);
                            let observerAttached = true;
                            const cleanup = () => {
                                if (!observerAttached) return;
                                observerAttached = false;
                                target.removeEventListener("faceplate-error", onLoadError);
                                target.removeEventListener("faceplate-alert", onLoadAlert);
                            };
                            controlState.cleanup = cleanup;
                            try {
                                const loading = target.loadContent();
                                if (loading && typeof loading.then === "function") {
                                    Promise.resolve(loading).then(cleanup, error => {
                                        cleanup();
                                        controlState.publicLoadErrorObserved = true;
                                        if (!isCurrent()) return;
                                        diagnostics.publicLoadRejected = true;
                                        diagnostics.publicLoadFailureName = publicLoadErrorName(error);
                                        diagnostics.publicLoadFailurePhase = "async";
                                        publish(activationStatus);
                                    });
                                } else {
                                    cleanup();
                                }
                            } catch (_) {
                                cleanup();
                                controlState.publicLoadErrorObserved = true;
                                diagnostics.publicLoadRejected = true;
                                diagnostics.publicLoadFailureName = publicLoadErrorName(_);
                                diagnostics.publicLoadFailurePhase = "sync";
                                publish(activationStatus);
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
            diagnostics.parentOwnedPartialCount = 0;
            diagnostics.parentOwnedLoadContentCount = 0;
            diagnostics.targetIsLoading = false;
            diagnostics.parentOwnedReplyButtonFound = false;
            publish("waiting_for_requested_comment_scope");
        }
        setTimeout(poll, 100);
    };

    poll();
})
