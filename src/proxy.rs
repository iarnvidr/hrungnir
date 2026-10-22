//! Per-launch loopback acquisition proxy. Public www Reddit HTML is captured
//! from HTTPS responses; asset hosts remain bounded, uncaptured tunnels.
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};

#[derive(Clone)]
pub struct BrowserProxy {
    pub address: SocketAddr,
    online: watch::Sender<bool>,
    cancellation: watch::Sender<u64>,
    generation: Arc<AtomicU64>,
    connects: Arc<AtomicU64>,
    tunnels: Arc<AtomicU64>,
    blocked: Arc<AtomicU64>,
    ca: Option<Arc<crate::ca::Ca>>,
    forwarder: Option<Arc<crate::response_capture::CaptureForwarder>>,
    captures:
        Arc<Mutex<Option<tokio::sync::mpsc::Receiver<crate::response_capture::CapturedResponse>>>>,
    failures:
        Arc<Mutex<Option<tokio::sync::mpsc::Receiver<crate::response_capture::CaptureFailure>>>>,
}

#[derive(Serialize)]
pub struct ProxyStatus {
    pub url: String,
    pub port: u16,
    pub online: bool,
    pub connects: u64,
    pub tunnels: u64,
    pub blocked: u64,
    pub capture: &'static str,
    pub response_stats: Option<serde_json::Value>,
}

pub fn allowed_host(host: &str) -> bool {
    [
        "reddit.com",
        "redditstatic.com",
        "redditmedia.com",
        "redd.it",
    ]
    .iter()
    .any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

impl BrowserProxy {
    pub async fn start() -> std::io::Result<Self> {
        Self::start_inner(None).await
    }

    pub async fn start_capture(directory: &std::path::Path) -> std::io::Result<Self> {
        Self::start_inner(Some(Arc::new(crate::ca::Ca::load_or_create(directory)?))).await
    }

    async fn start_inner(ca: Option<Arc<crate::ca::Ca>>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let (online, _) = watch::channel(true);
        let (cancellation, _) = watch::channel(0);
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        let (failure_sender, failure_receiver) = tokio::sync::mpsc::channel(8);
        let forwarder = if ca.is_some() {
            Some(Arc::new(crate::response_capture::CaptureForwarder::new(
                sender,
                failure_sender,
            )?))
        } else {
            None
        };
        let proxy = Self {
            address: listener.local_addr()?,
            online,
            cancellation,
            generation: Arc::default(),
            connects: Arc::default(),
            tunnels: Arc::default(),
            blocked: Arc::default(),
            ca,
            forwarder,
            captures: Arc::new(Mutex::new(Some(receiver))),
            failures: Arc::new(Mutex::new(Some(failure_receiver))),
        };
        let state = proxy.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let state = state.clone();
                let mut cancellation = state.cancellation.subscribe();
                let epoch = *cancellation.borrow();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = state.handle(socket, epoch) => {},
                        _ = cancellation.wait_for(|current| *current != epoch) => {},
                    }
                });
            }
        });
        Ok(proxy)
    }

    pub fn set_online(&self, online: bool) {
        self.online.send_replace(online);
        if !online {
            self.begin_acquisition();
        }
    }

    pub fn begin_acquisition(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.cancellation.send_replace(generation);
        generation
    }

    pub fn ca(&self) -> Option<Arc<crate::ca::Ca>> {
        self.ca.clone()
    }

    pub fn take_captures(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::response_capture::CapturedResponse>> {
        self.captures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn take_failures(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::response_capture::CaptureFailure>> {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn status(&self) -> ProxyStatus {
        ProxyStatus {
            port: self.address.port(),
            url: format!("http://{}", self.address),
            online: *self.online.borrow(),
            connects: self.connects.load(Ordering::Relaxed),
            tunnels: self.tunnels.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            capture: if self.ca.is_some() { "HTTPS HTML response tee; www.reddit.com only" } else { "uncaptured tunnel" },
            response_stats: self.forwarder.as_ref().map(|forwarder| {
                let stats = &forwarder.stats;
                let read = |value: &AtomicU64| value.load(Ordering::Relaxed);
                serde_json::json!({"requests":read(&stats.requests),"responses":read(&stats.responses),"captured_bytes":read(&stats.bytes),"dropped":read(&stats.dropped),"tls_ok":read(&stats.tls_ok),"tls_failed":read(&stats.tls_failed),"last_status":read(&stats.last_status)})
            }),
        }
    }

    async fn handle(&self, mut client: TcpStream, generation: u64) -> std::io::Result<()> {
        self.connects.fetch_add(1, Ordering::Relaxed);
        let mut head = Vec::new();
        // Read exactly to the end of CONNECT headers, preserving tunneled bytes.
        while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
            let byte = tokio::time::timeout(std::time::Duration::from_secs(10), client.read_u8())
                .await??;
            head.push(byte);
        }
        let text = String::from_utf8_lossy(&head);
        let mut parts = text.lines().next().unwrap_or_default().split_whitespace();
        let method = parts.next();
        let target = parts.next().unwrap_or_default();
        let valid = target.strip_suffix(":443").filter(|h| allowed_host(h));
        if method != Some("CONNECT") || valid.is_none() || !*self.online.borrow() {
            crate::diagnostics::event(
                "proxy.connect_blocked",
                serde_json::json!({"generation":generation,"reason":if !*self.online.borrow(){"offline"}else{"method_or_host"}}),
            );
            self.blocked.fetch_add(1, Ordering::Relaxed);
            client
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await?;
            return Ok(());
        }
        let host = valid.unwrap();
        crate::diagnostics::event(
            "proxy.connect",
            serde_json::json!({"generation":generation,"capture_host":host=="www.reddit.com"}),
        );
        if host == "www.reddit.com" {
            if let (Some(ca), Some(forwarder)) = (&self.ca, &self.forwarder) {
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;
                return forwarder
                    .clone()
                    .serve(client, ca.clone(), generation)
                    .await;
            }
        }
        // Remote media is acquired by the independent completed-file downloader.
        if host == "redd.it" || host.ends_with(".redd.it") {
            self.blocked.fetch_add(1, Ordering::Relaxed);
            client
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        }
        let addresses = tokio::net::lookup_host((host, 443)).await?;
        // The allowlist must never become a route into a private/local network.
        let public: Vec<_> = addresses.filter(|a| public_address(a.ip())).collect();
        let mut upstream = None;
        for address in public {
            if let Ok(Ok(stream)) = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                TcpStream::connect(address),
            )
            .await
            {
                upstream = Some(stream);
                break;
            }
        }
        let Some(mut upstream) = upstream else {
            client
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        };
        self.tunnels.fetch_add(1, Ordering::Relaxed);
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        Ok(())
    }
}

pub fn public_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        std::net::IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_address(mapped.into());
            }
            let segments = ip.segments();
            // Global unicast only, excluding IANA protocol-assignment and
            // documentation ranges. This also excludes multicast/local IPv6.
            (0x2000..=0x3fff).contains(&segments[0])
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && !(segments[0] == 0x3fff && segments[1] < 0x1000)
        }
    }
}
