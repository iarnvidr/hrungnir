//! Selective HTTPS response tee. Protocol/body handling follows Muninn's MIT
//! implementation; see licenses/MUNINN-MIT.txt. Only public www Reddit HTML is
//! copied; cookies, request bodies and authentication headers are never retained.
use bytes::Bytes;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::{
    body::{Body, Frame, Incoming, SizeHint},
    header::HeaderMap,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use sha2::{Digest, Sha256};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
};
use std::{
    convert::Infallible,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
};
use tokio::{net::TcpStream, sync::mpsc};
type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ResponseBody = UnsyncBoxBody<Bytes, BoxError>;
const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

fn search_request_cursor_fingerprint(query: Option<&str>) -> Option<String> {
    let mut cursor = None;
    for (key, value) in url::form_urlencoded::parse(query?.as_bytes()) {
        if key == "cursor" {
            if cursor.replace(value.into_owned()).is_some() {
                return None;
            }
        }
    }
    let cursor = cursor?;
    if cursor.is_empty() {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"hrungnir-search-source-cursor-v1\0");
    hash.update(cursor.as_bytes());
    Some(format!("{:x}", hash.finalize()))
}

fn safe_header_enum(headers: &HeaderMap, name: &'static str) -> &'static str {
    let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) else {
        return "missing";
    };
    let value = value.trim().to_ascii_lowercase();
    match name {
        "accept" => match value
            .split(',')
            .next()
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
        {
            "text/html" => "html",
            "application/xhtml+xml" => "xhtml",
            "application/json" => "json",
            "*/*" => "wildcard",
            _ => "other",
        },
        "sec-fetch-mode" => match value.as_str() {
            "navigate" => "navigate",
            "cors" => "cors",
            "no-cors" => "no-cors",
            "same-origin" => "same-origin",
            _ => "other",
        },
        "sec-fetch-dest" => match value.as_str() {
            "document" => "document",
            "empty" => "empty",
            "script" => "script",
            "style" => "style",
            "image" => "image",
            "font" => "font",
            _ => "other",
        },
        _ => "unsupported",
    }
}

fn safe_header_names(headers: &HeaderMap) -> Vec<String> {
    let mut names: Vec<_> = headers.keys().map(|name| name.as_str().to_owned()).collect();
    names.sort();
    names.dedup();
    names
}

pub struct CapturedResponse {
    pub generation: u64,
    pub request_id: u64,
    pub request_cursor_fingerprint: Option<[u8; 32]>,
    pub status_code: u16,
    pub content_type_kind: &'static str,
    pub url: String,
    pub encoding: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct CaptureFailure {
    pub generation: u64,
    pub error: String,
    pub retryable: bool,
}

#[derive(Default)]
pub struct CaptureStats {
    pub requests: AtomicU64,
    pub responses: AtomicU64,
    pub bytes: AtomicU64,
    pub dropped: AtomicU64,
    pub tls_ok: AtomicU64,
    pub tls_failed: AtomicU64,
    pub last_status: AtomicU64,
}

#[derive(Clone)]
struct PublicDns;
impl tower_service::Service<hyper_util::client::legacy::connect::dns::Name> for PublicDns {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = std::io::Error;
    type Future =
        Pin<Box<dyn std::future::Future<Output = std::io::Result<Self::Response>> + Send>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, name: hyper_util::client::legacy::connect::dns::Name) -> Self::Future {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| crate::proxy::public_address(a.ip()))
                .collect();
            if addresses.is_empty() {
                return Err(std::io::Error::other("No public upstream address"));
            }
            Ok(addresses.into_iter())
        })
    }
}

pub struct CaptureForwarder {
    client: Client<hyper_rustls::HttpsConnector<HttpConnector<PublicDns>>, ResponseBody>,
    pub stats: Arc<CaptureStats>,
    sender: mpsc::Sender<CapturedResponse>,
    failures: mpsc::Sender<CaptureFailure>,
}

impl CaptureForwarder {
    pub fn new(
        sender: mpsc::Sender<CapturedResponse>,
        failures: mpsc::Sender<CaptureFailure>,
    ) -> std::io::Result<Self> {
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(std::io::Error::other)?
        .with_root_certificates(rustls::RootCertStore::from_iter(
            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
        ))
        .with_no_client_auth();
        let mut http = HttpConnector::new_with_resolver(PublicDns);
        http.enforce_http(false);
        http.set_connect_timeout(Some(std::time::Duration::from_secs(15)));
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_only()
            .enable_http1()
            .wrap_connector(http);
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(https),
            stats: Arc::default(),
            sender,
            failures,
        })
    }

    pub async fn serve(
        self: Arc<Self>,
        socket: TcpStream,
        ca: Arc<crate::ca::Ca>,
        generation: u64,
    ) -> std::io::Result<()> {
        let acceptor = tokio_rustls::TlsAcceptor::from(ca.server_config("www.reddit.com")?);
        let started = std::time::Instant::now();
        let handshake =
            tokio::time::timeout(std::time::Duration::from_secs(15), acceptor.accept(socket)).await;
        let tls = match handshake {
            Ok(Ok(tls)) => tls,
            failed => {
                let (kind, io_kind, detail, retryable) = match failed {
                    Ok(Err(error)) => {
                        let retryable = matches!(
                            error.kind(),
                            std::io::ErrorKind::UnexpectedEof
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::BrokenPipe
                        );
                        (
                            "handshake",
                            format!("{:?}", error.kind()),
                            error.to_string(),
                            retryable,
                        )
                    }
                    Err(error) => ("timeout", "TimedOut".to_owned(), error.to_string(), false),
                    Ok(Ok(_)) => unreachable!(),
                };
                let message = format!("Source TLS {kind} failed: {detail}");
                self.stats.tls_failed.fetch_add(1, Ordering::Relaxed);
                let notified = self
                    .failures
                    .try_send(CaptureFailure {
                        generation,
                        error: message.clone(),
                        retryable,
                    })
                    .is_ok();
                crate::diagnostics::event(
                    "proxy.tls_failed",
                    serde_json::json!({"generation":generation,"elapsed_ms":started.elapsed().as_millis() as u64,"kind":kind,"io_kind":io_kind,"error":detail,"retryable":retryable,"failure_notified":notified}),
                );
                return Err(std::io::Error::other(message));
            }
        };
        self.stats.tls_ok.fetch_add(1, Ordering::Relaxed);
        crate::diagnostics::event(
            "proxy.tls_ok",
            serde_json::json!({"generation":generation,"elapsed_ms":started.elapsed().as_millis() as u64}),
        );
        let service = service_fn(move |request| self.clone().forward(request, generation));
        hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(tls), service)
            .await
            .map_err(std::io::Error::other)
    }

    async fn forward(
        self: Arc<Self>,
        request: Request<Incoming>,
        generation: u64,
    ) -> Result<Response<ResponseBody>, Infallible> {
        let request_id = self.stats.requests.fetch_add(1, Ordering::Relaxed) + 1;
        let (mut parts, body) = request.into_parts();
        let path = parts.uri.path();
        let comment_post = parts.method == Method::POST
            && crate::browsing::comment_more_partial_path(path, parts.uri.query())
            && parts
                .headers
                .get(hyper::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/x-www-form-urlencoded"));
        let search_partial = matches!(parts.method, Method::GET | Method::HEAD)
            && crate::browsing::search_partial_path(path, parts.uri.query());
        let mut request_cursor_fingerprint = None;
        if cfg!(debug_assertions)
            && std::env::var_os("HRUNGNIR_NATIVE_COMMENT_PROBE").is_some()
            && path.starts_with("/svc/shreddit/more-comments/")
        {
            let mut query_keys: Vec<_> = parts
                .uri
                .query()
                .into_iter()
                .flat_map(|query| url::form_urlencoded::parse(query.as_bytes()))
                .map(|(key, _)| key.into_owned())
                .collect();
            query_keys.sort();
            query_keys.dedup();
            eprintln!(
                "native comment probe request: method={}, path={}, query_keys={:?}",
                parts.method,
                path,
                query_keys
            );
            crate::diagnostics::event(
                "comment.request_observed",
                serde_json::json!({
                    "path":path,
                    "query_keys":query_keys,
                    "method":parts.method.as_str(),
                    "authority_matches":parts.uri.authority().is_none_or(|a| a.as_str()=="www.reddit.com"),
                }),
            );
        }
        // An acquisition worker has no reason to sign in or fetch private
        // endpoints. The single observed comment continuation is a bounded
        // public form POST; its values are forwarded transiently, never logged
        // or stored. A request's authority cannot change its CONNECT host.
        if !matches!(parts.method, Method::GET | Method::HEAD) && !comment_post
            || !(crate::browsing::public_content_path(path)
                || crate::browsing::listing_partial_path(path, parts.uri.query())
                || search_partial
                || comment_post)
            || parts
                .uri
                .authority()
                .is_some_and(|a| a.as_str() != "www.reddit.com")
        {
            return Ok(error_response(
                StatusCode::FORBIDDEN,
                "Only public community reads are enabled",
            ));
        }
        if cfg!(debug_assertions)
            && std::env::var_os("HRUNGNIR_NATIVE_PROBE").is_some()
            && search_partial
        {
            crate::diagnostics::event(
                "search.request_shape",
                serde_json::json!({
                    "generation":generation,
                    "request_id":request_id,
                    "cursor_fingerprint":search_request_cursor_fingerprint(parts.uri.query()),
                    "request_method":if parts.method == Method::GET { "get" } else { "head" },
                    "header_names":safe_header_names(&parts.headers),
                    "accept":safe_header_enum(&parts.headers,"accept"),
                    "sec_fetch_mode":safe_header_enum(&parts.headers,"sec-fetch-mode"),
                    "sec_fetch_dest":safe_header_enum(&parts.headers,"sec-fetch-dest"),
                }),
            );
        }
        let forwarded_body = if comment_post {
            let mut request_body = body;
            let mut bytes = Vec::new();
            while let Some(frame) = request_body.frame().await {
                let Ok(frame) = frame else {
                    return Ok(error_response(StatusCode::BAD_REQUEST, "Invalid comment continuation body"));
                };
                if let Ok(data) = frame.into_data() {
                    if bytes.len().saturating_add(data.len()) > 32 * 1024 {
                        return Ok(error_response(StatusCode::BAD_REQUEST, "Comment continuation body is too large"));
                    }
                    bytes.extend_from_slice(&data);
                }
            }
            if !crate::browsing::comment_more_form_body(&bytes) {
                return Ok(error_response(StatusCode::FORBIDDEN, "Invalid public comment continuation"));
            }
            request_cursor_fingerprint =
                crate::browsing::comment_more_form_cursor_fingerprint(&bytes);
            if request_cursor_fingerprint.is_none() {
                return Ok(error_response(StatusCode::FORBIDDEN, "Invalid public comment continuation"));
            }
            Full::new(Bytes::from(bytes))
                .map_err(|never| match never {})
                .boxed_unsync()
        } else {
            body.map_err(|error| Box::new(error) as BoxError).boxed_unsync()
        };
        let started = std::time::Instant::now();
        crate::diagnostics::event(
            "proxy.request",
            serde_json::json!({"generation":generation,"route":if path.contains("/comments/"){"post"}else{"listing"}}),
        );
        let target = format!(
            "https://www.reddit.com{}",
            parts
                .uri
                .path_and_query()
                .map(|p| p.as_str())
                .unwrap_or("/")
        );
        let capture = parts.method == Method::GET || comment_post;
        strip_hop_headers(&mut parts.headers);
        parts.headers.insert(
            hyper::header::HOST,
            hyper::header::HeaderValue::from_static("www.reddit.com"),
        );
        parts.headers.insert(
            hyper::header::ACCEPT_ENCODING,
            hyper::header::HeaderValue::from_static("gzip"),
        );
        // Force a full response so a browser's 304 cannot leave the structured
        // content store empty after eviction. No credential/header copy is kept.
        parts.headers.remove(hyper::header::IF_NONE_MATCH);
        parts.headers.remove(hyper::header::IF_MODIFIED_SINCE);
        let mut upstream = Request::from_parts(parts, forwarded_body);
        *upstream.uri_mut() = target.parse().expect("fixed public upstream URL");
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(25),
            self.client.request(upstream),
        )
        .await
        {
            Ok(Ok(response)) => response,
            _ => {
                crate::diagnostics::event(
                    "proxy.upstream_failed",
                    serde_json::json!({"generation":generation,"elapsed_ms":started.elapsed().as_millis() as u64}),
                );
                return Ok(error_response(
                    StatusCode::BAD_GATEWAY,
                    "Public source request failed",
                ));
            }
        };
        let (mut parts, body) = response.into_parts();
        crate::diagnostics::event(
            "proxy.headers",
            serde_json::json!({"generation":generation,"status":parts.status.as_u16(),"elapsed_ms":started.elapsed().as_millis() as u64}),
        );
        self.stats
            .last_status
            .store(parts.status.as_u16() as u64, Ordering::Relaxed);
        eprintln!(
            "proxy source response: status={}, generation={generation}",
            parts.status.as_u16()
        );
        let content_type_kind = parts
            .headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase())
            .map(|value| match value.as_str() {
                "text/html" => "html",
                "text/vnd.reddit.partial+html" => "reddit_partial_html",
                "application/json" => "json",
                "" => "missing",
                _ if value.ends_with("+json") => "json",
                _ => "other",
            })
            .unwrap_or("missing");
        let html = matches!(content_type_kind, "html" | "reddit_partial_html");
        let status_code = parts.status.as_u16();
        let encoding = parts
            .headers
            .get(hyper::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        strip_hop_headers(&mut parts.headers);
        parts.headers.remove("alt-svc");
        // SQLite is the content cache. A future acquisition must traverse the
        // proxy instead of being satisfied by the worker's HTTP disk cache.
        parts.headers.insert(
            hyper::header::CACHE_CONTROL,
            hyper::header::HeaderValue::from_static("no-store"),
        );
        let inner = body.map_err(|e| Box::new(e) as BoxError).boxed_unsync();
        let body = if capture && html && parts.status == StatusCode::OK {
            let stats = self.stats.clone();
            let sender = self.sender.clone();
            TeeBody {
                inner,
                buf: Vec::new(),
                cap: CAPTURE_LIMIT,
                overflow: false,
                on_done: Some(Box::new(move |bytes| {
                    crate::diagnostics::event("proxy.body_complete",serde_json::json!({"generation":generation,"bytes":bytes.len(),"elapsed_ms":started.elapsed().as_millis() as u64}));
                    stats.responses.fetch_add(1, Ordering::Relaxed);
                    stats.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    if sender
                        .try_send(CapturedResponse {
                            generation,
                            request_id,
                            request_cursor_fingerprint,
                            status_code,
                            content_type_kind,
                            url: target,
                            encoding,
                            bytes,
                        })
                        .is_err()
                    {
                        stats.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                })),
            }
            .boxed_unsync()
        } else {
            inner
        };
        Ok(Response::from_parts(parts, body))
    }
}

fn error_response(status: StatusCode, message: &'static str) -> Response<ResponseBody> {
    let mut response = Response::new(
        Full::new(Bytes::from_static(message.as_bytes()))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = status;
    response
}

fn strip_hop_headers(headers: &mut HeaderMap) {
    let listed: Vec<_> = headers
        .get_all(hyper::header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|part| part.trim().to_owned()))
        .collect();
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
    for name in listed {
        headers.remove(name);
    }
}

// Streaming tee adapted from Muninn. Errors and oversized responses never
// publish a partial capture; all original frames still go to the webview.
struct TeeBody {
    inner: ResponseBody,
    buf: Vec<u8>,
    cap: usize,
    overflow: bool,
    on_done: Option<Box<dyn FnOnce(Vec<u8>) + Send>>,
}
impl TeeBody {
    fn finish(&mut self) {
        if let Some(done) = self.on_done.take() {
            if !self.overflow {
                done(std::mem::take(&mut self.buf));
            }
        }
    }
}
impl Body for TeeBody {
    type Data = Bytes;
    type Error = BoxError;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(bytes) = frame.data_ref() {
                    if !this.overflow {
                        if this.buf.len() + bytes.len() > this.cap {
                            this.overflow = true;
                            this.buf.clear();
                        } else {
                            this.buf.extend_from_slice(bytes);
                        }
                    }
                }
                if this.inner.is_end_stream() {
                    this.finish();
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(None) => {
                this.finish();
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(error))) => {
                this.on_done.take();
                Poll::Ready(Some(Err(error)))
            }
            other => other,
        }
    }
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    struct Frames(VecDeque<Result<Frame<Bytes>, BoxError>>);
    impl Body for Frames {
        type Data = Bytes;
        type Error = BoxError;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            Poll::Ready(self.0.pop_front())
        }
        fn is_end_stream(&self) -> bool {
            self.0.is_empty()
        }
    }
    fn tee(
        frames: Vec<Result<Frame<Bytes>, BoxError>>,
        cap: usize,
        published: Arc<Mutex<Option<Vec<u8>>>>,
    ) -> TeeBody {
        TeeBody {
            inner: Frames(frames.into()).boxed_unsync(),
            buf: Vec::new(),
            cap,
            overflow: false,
            on_done: Some(Box::new(move |bytes| {
                *published.lock().unwrap() = Some(bytes)
            })),
        }
    }
    #[tokio::test]
    async fn tee_preserves_frames_and_publishes_only_complete_bounded_bodies() {
        let published = Arc::new(Mutex::new(None));
        let body = tee(
            vec![
                Ok(Frame::data(Bytes::from_static(b"abc"))),
                Ok(Frame::data(Bytes::from_static(b"def"))),
            ],
            6,
            published.clone(),
        );
        assert_eq!(
            body.collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"abcdef")
        );
        assert_eq!(
            published.lock().unwrap().as_deref(),
            Some(b"abcdef".as_slice())
        );
        let published = Arc::new(Mutex::new(None));
        let body = tee(
            vec![Ok(Frame::data(Bytes::from_static(b"abcdef")))],
            5,
            published.clone(),
        );
        assert_eq!(
            body.collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"abcdef")
        );
        assert!(published.lock().unwrap().is_none());
    }
    #[tokio::test]
    async fn failed_or_abandoned_response_never_publishes_a_partial_capture() {
        let published = Arc::new(Mutex::new(None));
        let body = tee(
            vec![
                Ok(Frame::data(Bytes::from_static(b"abc"))),
                Err(std::io::Error::other("interrupted").into()),
            ],
            10,
            published.clone(),
        );
        assert!(body.collect().await.is_err());
        assert!(published.lock().unwrap().is_none());
        let published = Arc::new(Mutex::new(None));
        let mut body = tee(
            vec![
                Ok(Frame::data(Bytes::from_static(b"abc"))),
                Ok(Frame::data(Bytes::from_static(b"def"))),
            ],
            10,
            published.clone(),
        );
        assert!(body.frame().await.unwrap().is_ok());
        drop(body);
        assert!(published.lock().unwrap().is_none());
    }

    #[test]
    fn search_request_diagnostic_hashes_only_the_decoded_cursor() {
        let encoded = search_request_cursor_fingerprint(Some(
            "cursor=page%2Bone&q=private+query&sort=relevance&t=all",
        ));
        let decoded = search_request_cursor_fingerprint(Some(
            "q=different+private+query&cursor=page%2Bone&sort=relevance&t=all",
        ));
        let advanced = search_request_cursor_fingerprint(Some(
            "cursor=page%2Btwo&q=private+query&sort=relevance&t=all",
        ));
        assert_eq!(encoded, decoded);
        assert_ne!(encoded, advanced);
        assert!(search_request_cursor_fingerprint(Some("cursor=&q=x")).is_none());
        assert!(search_request_cursor_fingerprint(Some("cursor=a&cursor=b&q=x")).is_none());
        assert!(search_request_cursor_fingerprint(None).is_none());
    }

    #[test]
    fn search_request_diagnostic_exposes_names_and_enum_classes_not_header_values() {
        let mut headers = HeaderMap::new();
        headers.insert("accept", "text/html,application/xhtml+xml".parse().unwrap());
        headers.insert("sec-fetch-mode", "cors".parse().unwrap());
        headers.insert("sec-fetch-dest", "empty".parse().unwrap());
        headers.insert("cookie", "private-cookie-value".parse().unwrap());
        headers.insert("referer", "https://www.reddit.com/private-path".parse().unwrap());
        headers.insert("authorization", "Bearer private-token".parse().unwrap());

        let names = safe_header_names(&headers);
        assert_eq!(safe_header_enum(&headers, "accept"), "html");
        assert_eq!(safe_header_enum(&headers, "sec-fetch-mode"), "cors");
        assert_eq!(safe_header_enum(&headers, "sec-fetch-dest"), "empty");
        let diagnostic = serde_json::to_string(&serde_json::json!({
            "header_names": names,
            "accept": safe_header_enum(&headers, "accept"),
            "sec_fetch_mode": safe_header_enum(&headers, "sec-fetch-mode"),
            "sec_fetch_dest": safe_header_enum(&headers, "sec-fetch-dest"),
        })).unwrap();
        assert!(diagnostic.contains("cookie"));
        assert!(diagnostic.contains("authorization"));
        assert!(!diagnostic.contains("private-cookie-value"));
        assert!(!diagnostic.contains("private-path"));
        assert!(!diagnostic.contains("private-token"));
        assert!(!diagnostic.contains("text/html"));
    }
}
