//! Invisible acquisition worker. Remote website content never becomes the UI.
use reddit_offline::{
    browsing::{classic_url, listing_url, reddit_url, PageCapture},
    desktop::DesktopStore,
    proxy::BrowserProxy,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const WORKER: &str = "reddit-live";
struct Progress {
    generation: u64,
    url: String,
    revision: u64,
    error: Option<String>,
    captured: bool,
    ids: Vec<String>,
    layout: Option<String>,
    preference_present: bool,
    /// A listing's remaining posts arrive in a follow-up response; until this
    /// instant the acquisition keeps waiting for it.
    more_until: Option<Instant>,
}
pub struct Browser {
    app: AppHandle,
    pub store: DesktopStore,
    proxy: BrowserProxy,
    progress: Mutex<Progress>,
    pub gate: tokio::sync::Mutex<()>,
    pub trust_gate: tokio::sync::Mutex<()>,
    trust_recovery: Mutex<serde_json::Value>,
    operations: Mutex<VecDeque<serde_json::Value>>,
    active: Mutex<BTreeMap<String, (u64, u64)>>,
    ready: tokio::sync::Notify,
}

impl Browser {
    pub fn new(app: AppHandle, store: DesktopStore, proxy: BrowserProxy) -> Arc<Self> {
        proxy.set_online(false);
        let captures = proxy.take_captures();
        let failures = proxy.take_failures();
        let browser = Arc::new(Self {
            app,
            store,
            proxy,
            progress: Mutex::new(Progress {
                generation: 0,
                url: String::new(),
                revision: 0,
                error: None,
                captured: false,
                ids: Vec::new(),
                layout: None,
                preference_present: false,
                more_until: None,
            }),
            gate: tokio::sync::Mutex::new(()),
            trust_gate: tokio::sync::Mutex::new(()),
            trust_recovery: Mutex::new(
                serde_json::json!({"available":cfg!(target_os="macos"),"state":"delegate_only","detail":null}),
            ),
            operations: Mutex::new(VecDeque::new()),
            active: Mutex::new(BTreeMap::new()),
            ready: tokio::sync::Notify::new(),
        });
        if let Some(mut failures) = failures {
            let weak = Arc::downgrade(&browser);
            tauri::async_runtime::spawn(async move {
                while let Some(failure) = failures.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    if failure.retryable {
                        // WebKit may abandon a certificate-probe connection and
                        // reconnect after its trust challenge is accepted. A
                        // single socket EOF is not a failed page acquisition.
                        reddit_offline::diagnostics::event(
                            "source.connection_retry",
                            serde_json::json!({"generation":failure.generation,"error":failure.error,"deadline":"existing acquisition deadline"}),
                        );
                    } else {
                        browser.fail_generation(failure.generation, &failure.error);
                    }
                }
            });
        }
        if let Some(mut captures) = captures {
            let weak = Arc::downgrade(&browser);
            tauri::async_runtime::spawn(async move {
                while let Some(response) = captures.recv().await {
                    let Some(browser) = weak.upgrade() else {
                        break;
                    };
                    tauri::async_runtime::spawn_blocking(move || {
                        let generation = response.generation;
                        let started = Instant::now();
                        browser.record_detail("parse", "running", 0, None, serde_json::json!({"generation":generation,"compressed_bytes":response.bytes.len()}));
                        eprintln!(
                            "proxy capture: generation={generation}, compressed_bytes={}",
                            response.bytes.len()
                        );
                        let partial = reddit_offline::browsing::is_listing_partial_url(&response.url);
                        let parsed = if partial {
                            let (listing, taken) = {
                                let progress = browser.progress.lock().unwrap_or_else(|e| e.into_inner());
                                (progress.url.clone(), progress.ids.len())
                            };
                            reddit_offline::reddit_html::parse_listing_partial(response, &listing, 25usize.saturating_sub(taken))
                        } else {
                            reddit_offline::reddit_html::parse(response)
                        };
                        let result = parsed
                            .map_err(|error| error.to_string())
                            .and_then(|page| {
                                let actual = page
                                    .url
                                    .parse::<tauri::Url>()
                                    .map_err(|error| error.to_string())?;
                                let json = serde_json::to_string(&page)
                                    .map_err(|error| error.to_string())?;
                                let more = page.more_url.clone();
                                let count = browser.capture_page(&json, actual, generation, partial, more.is_some())?;
                                if let Some(more) = more {
                                    browser.request_more(generation, &more);
                                }
                                Ok(count)
                            });
                        if let Err(error) = result {
                            eprintln!("proxy capture rejected: {error}");
                            if partial {
                                // The listing itself is already stored: a missing
                                // continuation only means fewer posts.
                                browser.stop_waiting_for_more(generation);
                            } else if error != "Expired acquisition request"
                                && !reddit_offline::reddit_html::is_interim(&error)
                            {
                                browser.fail_generation(generation,&error);
                            }
                            browser.record_detail("parse", "failed", started.elapsed().as_millis() as u64, Some(error),serde_json::json!({"generation":generation}));
                        } else {
                            browser.record_detail("parse", "complete", started.elapsed().as_millis() as u64,None,serde_json::json!({"generation":generation}));
                        }
                    })
                    .await
                    .ok();
                }
            });
        }
        browser
    }

    fn capture_page(
        &self,
        json: &str,
        mut actual: tauri::Url,
        generation: u64,
        partial: bool,
        more_expected: bool,
    ) -> Result<usize, String> {
        if json.len() > 512 * 1024 {
            return Err("Capture exceeds 512 KiB".into());
        }
        if !reddit_offline::browsing::search_path(actual.path()) { actual.set_query(None); }
        actual.set_fragment(None);
        let page: PageCapture = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !reddit_url(actual.as_str()) || page.url != actual.as_str() {
            return Err("Capture URL mismatch".into());
        }
        let mut progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if progress.url != page.url || progress.generation != generation {
            return Err("Expired acquisition request".into());
        }
        let started = Instant::now();
        let count = self.store.capture_page(json).map_err(|e| e.to_string())?;
        self.record_detail("cache_commit", "complete", started.elapsed().as_millis() as u64,None,serde_json::json!({"generation":generation,"posts":page.threads.len(),"comments":page.threads.iter().map(|t|t.comments.len()).sum::<usize>()}));
        eprintln!(
            "source observation: layout={}, posts={}, classic_preference_present={}",
            page.layout.as_deref().unwrap_or("unspecified"),
            page.threads.len(),
            progress.preference_present
        );
        if partial {
            for thread in &page.threads {
                if !progress.ids.contains(&thread.id) {
                    progress.ids.push(thread.id.clone());
                }
            }
            progress.more_until = None;
            self.ready.notify_one();
            return Ok(count);
        }
        progress.more_until = more_expected.then(|| Instant::now() + Duration::from_secs(6));
        progress.ids = page
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect();
        progress.layout = page.layout.clone();
        if let Some(present) = page.layout_preference {
            progress.preference_present = present;
        }
        let current = actual
            .path()
            .split('/')
            .nth(4)
            .filter(|_| actual.path().contains("/comments/"))
            .map(|id| format!("t3_{id}"));
        progress.captured = reddit_offline::browsing::search_path(actual.path()) || page.threads.iter().any(|thread| {
            current.as_ref().is_none_or(|id| {
                thread.id == *id && reddit_offline::browsing::comments_ready(thread)
            })
        });
        progress.revision += 1;
        self.ready.notify_one();
        Ok(count)
    }

    fn close(&self) -> Result<tokio::sync::oneshot::Receiver<Result<(), String>>, String> {
        let (done, finished) = tokio::sync::oneshot::channel();
        if let Some(view) = self.app.get_webview_window(WORKER) {
            let destination = view.clone();
            view.with_webview(move |native| {
                #[cfg(target_os = "linux")]
                {
                    use webkit2gtk::WebViewExt;
                    native.inner().stop_loading();
                }
                #[cfg(target_os = "macos")]
                unsafe {
                    let view = native.inner() as *mut objc2::runtime::AnyObject;
                    let _: () = objc2::msg_send![view, stopLoading];
                }
                let _ = done.send(destination.destroy().map_err(|error| error.to_string()));
            })
            .map_err(|e| e.to_string())?;
        } else {
            let _ = done.send(Ok(()));
        }
        Ok(finished)
    }

    fn start(self: &Arc<Self>, url: String) -> Result<(), String> {
        let generation = self.proxy.begin_acquisition();
        self.proxy.set_online(true);
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Progress {
            generation,
            url: url.clone(),
            revision: 0,
            error: None,
            captured: false,
            ids: Vec::new(),
            layout: None,
            preference_present: false,
            more_until: None,
        };
        let navigation = self.clone();
        let builder = WebviewWindowBuilder::new(
            &self.app,
            WORKER,
            // Set the public layout preference before the first Reddit request.
            WebviewUrl::External("about:blank".parse().unwrap()),
        )
        .title("Background acquisition")
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .inner_size(1000.0, 900.0)
        .proxy_url(
            format!("http://{}", self.proxy.address)
                .parse::<tauri::Url>()
                .map_err(|e| e.to_string())?,
        )
        .initialization_script(
            "HTMLMediaElement.prototype.play=function(){this.pause();return Promise.resolve()};",
        )
        .on_page_load(|_view, payload| {
            reddit_offline::diagnostics::event("source.page_load",serde_json::json!({"state":format!("{:?}",payload.event()),"host":payload.url().host_str().unwrap_or("blank")}));
            eprintln!(
                "source load: {:?}, host={}",
                payload.event(),
                payload.url().host_str().unwrap_or("blank")
            );
        })
        .on_navigation(move |url| {
            if url.as_str() == "about:blank" {
                return true;
            }
            let mut progress = navigation
                .progress
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if progress.generation != generation || progress.url.is_empty() {
                return false;
            }
            let allowed = reddit_url(url.as_str())
                && reddit_offline::browsing::public_content_path(url.path());
            if reddit_url(url.as_str()) && !allowed {
                progress.error =
                    Some("Reddit redirected this source away from public community content".into());
                navigation.ready.notify_one();
            }
            if allowed {
                let mut actual = url.clone();
                if !reddit_offline::browsing::search_path(actual.path()) {
                    actual.set_query(None);
                } else if let Ok(clean) =
                    reddit_offline::browsing::without_challenge_params(actual.as_str())
                        .parse::<tauri::Url>()
                {
                    // Reddit's verification step resubmits the search with extra fields.
                    actual = clean;
                }
                actual.set_fragment(None);
                progress.url = actual.to_string();
            }
            allowed
        });
        #[cfg(target_os = "macos")]
        let builder = builder.data_store_identifier(*b"OfflineReddit001");
        #[cfg(target_os = "linux")]
        let builder = builder.data_directory(
            self.app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("reddit-webdata"),
        );
        let view = builder.build().map_err(|e| e.to_string())?;
        if let Some(ca) = self.proxy.ca() {
            #[cfg(target_os = "macos")]
            {
                let browser = self.clone();
                super::trust_mac::prepare(&ca, &view, move |error| {
                    browser.fail_generation(generation, &error)
                })?;
            }
            #[cfg(target_os = "linux")]
            {
                let pem = ca
                    .leaf_pem("www.reddit.com")
                    .ok_or("Could not prepare source certificate")?;
                view.with_webview(move |native| {
                    use webkit2gtk::{WebContextExt, WebViewExt};
                    if let (Some(context), Ok(certificate)) = (
                        native.inner().context(),
                        webkit2gtk::gio::TlsCertificate::from_pem(&pem),
                    ) {
                        #[allow(deprecated)]
                        context.allow_tls_certificate_for_host(&certificate, "www.reddit.com");
                    }
                })
                .map_err(|e| e.to_string())?;
            }
        }
        self.navigate_with_preference(view, url, generation)?;
        Ok(())
    }

    fn navigate_with_preference(
        self: &Arc<Self>,
        view: tauri::WebviewWindow,
        url: String,
        generation: u64,
    ) -> Result<(), String> {
        let browser = self.clone();
        // Use the platform completion callback: synchronous wrappers can run
        // nested main loops and race navigation or deadlock on GTK.
        view.with_webview(move |native| {
            eprintln!("classic preference: native store initialization");
            #[cfg(target_os = "linux")]
            {
                use webkit2gtk::{
                    CookieManagerExt, URIRequestExt, WebViewExt, WebsiteDataManagerExt,
                };
                let Some(manager) = native
                    .inner()
                    .website_data_manager()
                    .and_then(|data| data.cookie_manager())
                else {
                    browser.fail_generation(generation, "Native cookie store is unavailable");
                    return;
                };
                let mut preference =
                    soup::Cookie::new("redesign_optout", "true", ".reddit.com", "/", 63072000);
                preference.set_secure(true);
                preference.set_same_site_policy(soup::SameSitePolicy::Lax);
                let target = native.inner().clone();
                manager.add_cookie(
                    &mut preference,
                    None::<&webkit2gtk::gio::Cancellable>,
                    move |result| {
                        eprintln!(
                            "classic preference: store write completed, success={}",
                            result.is_ok()
                        );
                        if let Err(error) = result {
                            browser.fail_generation(
                                generation,
                                &format!("Could not save classic layout preference: {error}"),
                            );
                            return;
                        }
                        if !browser.preference_ready(generation) {
                            return;
                        }
                        let request = webkit2gtk::URIRequest::new(&url);
                        if let Some(headers) = request.http_headers() {
                            headers.replace("Cache-Control", "no-cache");
                            headers.replace("Pragma", "no-cache");
                        }
                        target.load_request(&request);
                    },
                );
            }
            #[cfg(target_os = "macos")]
            unsafe {
                use objc2::{msg_send, runtime::AnyObject};
                use objc2_foundation::{
                    NSDate, NSDictionary, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieExpires,
                    NSHTTPCookieName, NSHTTPCookiePath, NSHTTPCookieSecure, NSHTTPCookieValue,
                    NSString, NSURLRequest, NSURLRequestCachePolicy, NSURL,
                };
                let name = NSString::from_str("redesign_optout");
                let value = NSString::from_str("true");
                let domain = NSString::from_str(".reddit.com");
                let path = NSString::from_str("/");
                let secure = NSString::from_str("TRUE");
                let expires = NSDate::dateWithTimeIntervalSinceNow(63072000.0);
                let properties: objc2::rc::Retained<NSDictionary<NSString, AnyObject>> =
                    NSDictionary::from_slices(
                        &[
                            NSHTTPCookieName,
                            NSHTTPCookieValue,
                            NSHTTPCookieDomain,
                            NSHTTPCookiePath,
                            NSHTTPCookieSecure,
                            NSHTTPCookieExpires,
                        ],
                        &[&name, &value, &domain, &path, &secure, &expires],
                    );
                let Some(preference) = NSHTTPCookie::cookieWithProperties(&properties) else {
                    browser
                        .fail_generation(generation, "Could not create classic layout preference");
                    return;
                };
                let configuration: *mut AnyObject =
                    msg_send![native.inner() as *mut AnyObject, configuration];
                let data: *mut AnyObject = msg_send![configuration, websiteDataStore];
                let cookies: *mut AnyObject = msg_send![data, httpCookieStore];
                let target =
                    objc2::rc::Retained::<AnyObject>::retain(native.inner() as *mut AnyObject)
                        .expect("live native source view");
                let done = block2::RcBlock::new(move || {
                    if !browser.preference_ready(generation) {
                        return;
                    }
                    let source = NSURL::URLWithString(&NSString::from_str(&url))
                        .expect("validated source URL");
                    let request = NSURLRequest::requestWithURL_cachePolicy_timeoutInterval(
                        &source,
                        NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
                        25.0,
                    );
                    let _: *mut AnyObject = msg_send![&*target, loadRequest: &*request];
                });
                let _: () = msg_send![cookies, setCookie: &*preference, completionHandler: &*done];
            }
        })
        .map_err(|e| e.to_string())
    }

    fn fail_generation(&self, generation: u64, error: &str) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation == generation && !progress.url.is_empty() {
            progress.error = Some(error.to_owned());
            self.ready.notify_one();
        }
    }

    /// The page's own script requests a listing's remaining posts when it
    /// scrolls into view, which a hidden window may never do. Ask for them
    /// from inside the page so the request, cookies and capture are the same.
    fn request_more(&self, generation: u64, path_and_query: &str) {
        if !reddit_offline::browsing::listing_partial_path(
            path_and_query.split('?').next().unwrap_or_default(),
            path_and_query.split_once('?').map(|(_, query)| query),
        ) {
            return;
        }
        {
            let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            if progress.generation != generation || progress.url.is_empty() {
                return;
            }
        }
        let Ok(target) = serde_json::to_string(path_and_query) else {
            return;
        };
        if let Some(view) = self.app.get_webview_window(WORKER) {
            let script = format!(
                "fetch({target},{{credentials:'include'}}).then(r=>r.text()).catch(()=>{{}});"
            );
            if let Err(error) = view.eval(script.as_str()) {
                eprintln!("listing continuation request failed: {error}");
            }
        }
    }

    fn stop_waiting_for_more(&self, generation: u64) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation == generation {
            progress.more_until = None;
            self.ready.notify_one();
        }
    }

    fn preference_ready(&self, generation: u64) -> bool {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.generation != generation || progress.url.is_empty() {
            return false;
        }
        progress.preference_present = true;
        reddit_offline::diagnostics::event(
            "source.preference_ready",
            serde_json::json!({"generation":generation}),
        );
        true
    }

    pub async fn acquire(self: &Arc<Self>, url: String, _post: bool) -> Result<(), String> {
        if !reddit_url(&url)
            || !reddit_offline::browsing::public_content_path(url.parse::<tauri::Url>().map_err(|e|e.to_string())?.path())
        {
            return Err("Only public Reddit listing, post or search content can be acquired".into());
        }
        // The layout preference cookie is still set on www, but Reddit now
        // serves its current layout there and old.reddit.com redirects away.
        let classic = classic_url(&url).map_err(|e| e.to_string())?;
        let browser = self.clone();
        let started = Instant::now();
        self.record("source", "running", 0, None);
        let setup = Instant::now();
        self.record("source_startup", "running", 0, None);
        if let Err(error) = super::on_main(self.app.clone(), move || browser.start(classic)).await {
            let _ = self.stop().await;
            self.record(
                "source_startup",
                "failed",
                setup.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            self.record("source", "failed", 0, Some(error.clone()));
            return Err(error);
        }
        self.record(
            "source_startup",
            "complete",
            setup.elapsed().as_millis() as u64,
            None,
        );
        let result = loop {
            let mut deadline = started + Duration::from_secs(25);
            {
                let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(error) = &progress.error {
                    break Err(error.clone());
                }
                // A response is published only after a complete body and atomic
                // parse/commit. There is no DOM settling interval to wait for.
                if progress.captured {
                    // The posts are stored; only a pending listing continuation
                    // is worth a short extra wait.
                    match progress.more_until {
                        Some(until) if Instant::now() < until => deadline = deadline.min(until),
                        _ => break Ok(()),
                    }
                }
            }
            if tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                self.ready.notified(),
            )
            .await
            .is_err()
            {
                if self.progress.lock().unwrap_or_else(|e| e.into_inner()).captured {
                    continue;
                }
                break Err(
                    "Reddit acquisition timed out; cached content is preserved".into(),
                );
            }
        };
        let cleanup = self.stop().await;
        let result = result.and(cleanup);
        eprintln!(
            "acquisition: classic www source, elapsed_ms={}, success={}",
            started.elapsed().as_millis(),
            result.is_ok()
        );
        self.record(
            "source",
            if result.is_ok() { "complete" } else { "failed" },
            started.elapsed().as_millis() as u64,
            result.as_ref().err().cloned(),
        );
        result
    }

    async fn stop(self: &Arc<Self>) -> Result<(), String> {
        let started = Instant::now();
        self.record("source_teardown", "running", 0, None);
        let browser = self.clone();
        let result = match super::on_main(self.app.clone(), move || browser.close()).await {
            Ok(finished) => match tokio::time::timeout(Duration::from_secs(2), finished).await {
                Ok(Ok(result)) => result,
                _ => Err("Native source shutdown timed out".to_owned()),
            },
            Err(error) => Err(error),
        };
        // The native load has stopped before normal cancellation. Every failure
        // path still turns forwarding off and invalidates queued captures.
        self.proxy.set_online(false);
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .url
            .clear();
        if let Err(error) = result {
            self.record(
                "source_teardown",
                "failed",
                started.elapsed().as_millis() as u64,
                Some(error.clone()),
            );
            return Err(error);
        }
        for _ in 0..40 {
            if self.app.get_webview_window(WORKER).is_none() {
                self.record(
                    "source_teardown",
                    "complete",
                    started.elapsed().as_millis() as u64,
                    None,
                );
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.record(
            "source_teardown",
            "failed",
            started.elapsed().as_millis() as u64,
            Some("Background source did not close cleanly".into()),
        );
        Err("Background source did not close cleanly".into())
    }

    pub async fn subreddit(
        self: &Arc<Self>,
        subreddit: &str,
        sort: &str,
        period: &str,
    ) -> Result<Vec<String>, String> {
        self.acquire(
            listing_url(subreddit, sort, period).map_err(|e| e.to_string())?,
            false,
        )
        .await?;
        Ok(self
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ids
            .clone())
    }

    pub async fn search(self: &Arc<Self>, url: String) -> Result<Vec<String>, String> {
        self.acquire(url, false).await?;
        Ok(self.progress.lock().unwrap_or_else(|e|e.into_inner()).ids.clone())
    }

    pub fn record(&self, stage: &str, status: &str, elapsed_ms: u64, error: Option<String>) {
        self.record_detail(stage, status, elapsed_ms, error, serde_json::json!({}));
    }

    pub fn trust_ca(&self) -> Option<Arc<reddit_offline::ca::Ca>> {
        self.proxy.ca()
    }
    pub fn trust_recovery_status(&self) -> serde_json::Value {
        self.trust_recovery
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_trust_recovery(&self, state: &str, detail: Option<String>) {
        *self
            .trust_recovery
            .lock()
            .unwrap_or_else(|e| e.into_inner()) =
            serde_json::json!({"available":cfg!(target_os="macos"),"state":state,"detail":detail});
        reddit_offline::diagnostics::event(
            "macos.trust_recovery",
            serde_json::json!({"state":state,"detail":detail}),
        );
    }

    pub fn record_detail(
        &self,
        stage: &str,
        status: &str,
        elapsed_ms: u64,
        error: Option<String>,
        details: serde_json::Value,
    ) {
        let now = reddit_offline::diagnostics::now_ms();
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if status == "running" {
            let entry = active.entry(stage.to_owned()).or_insert((now, 0));
            entry.1 += 1;
        } else if let Some(entry) = active.get_mut(stage) {
            entry.1 = entry.1.saturating_sub(1);
            if entry.1 == 0 {
                active.remove(stage);
            }
        }
        drop(active);
        let event = serde_json::json!({"timestamp_ms":now,"stage":stage,"status":status,"elapsed_ms":elapsed_ms,"error":error,"details":details});
        reddit_offline::diagnostics::event("operation", event.clone());
        let mut events = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        events.push_back(event);
        while events.len() > 100 {
            events.pop_front();
        }
    }

    pub fn diagnostics(&self) -> serde_json::Value {
        let progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        let now = reddit_offline::diagnostics::now_ms();
        let active:Vec<_>=self.active.lock().unwrap_or_else(|e|e.into_inner()).iter().map(|(stage,(started,count))|serde_json::json!({"stage":stage,"started_at_ms":started,"elapsed_ms":now.saturating_sub(*started),"count":count})).collect();
        serde_json::json!({"source":{"host":"www.reddit.com","transport":"HTTPS response tee","parser":"Reddit HTML in Rust","trust":"app-scoped, exact www.reddit.com host","layout":"current","last_observed_layout":progress.layout,"preference_configured":progress.preference_present,"last_capture_posts":progress.ids.len(),"acquisition_busy":self.gate.try_lock().is_err(),"remote_ui":false,"comments":"downloaded HTML subset, maximum 300"},"proxy":self.proxy.status(),"operations":self.operations.lock().unwrap_or_else(|e| e.into_inner()).clone(),"active_operations":active,"logging":reddit_offline::diagnostics::health()})
    }
}
