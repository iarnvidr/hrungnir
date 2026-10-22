//! Loopback-only, per-launch tokenized serving of completed media, with ranges.
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub struct MediaServer {
    pub base: String,
}

impl MediaServer {
    pub async fn start(root: PathBuf) -> std::io::Result<Self> {
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|error| std::io::Error::other(error.to_string()))?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let base = format!("http://{}/{token}", listener.local_addr()?);
        let permits = Arc::new(tokio::sync::Semaphore::new(16));
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    continue;
                };
                let root = root.clone();
                let token = token.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(120),
                        serve(socket, root, &token),
                    )
                    .await;
                });
            }
        });
        Ok(Self { base })
    }
}

pub fn range(value: Option<&str>, length: u64) -> Option<(u64, u64, bool)> {
    if length == 0 {
        return None;
    }
    let Some(value) = value else {
        return Some((0, length - 1, false));
    };
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((length.saturating_sub(suffix), length - 1, true));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?.min(length - 1)
    };
    (start <= end && start < length).then_some((start, end, true))
}

async fn error(socket: &mut TcpStream, status: &str, extra: &str) -> std::io::Result<()> {
    socket
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n{extra}\r\n")
                .as_bytes(),
        )
        .await
}

async fn serve(mut socket: TcpStream, root: PathBuf, token: &str) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 8192 {
            return error(&mut socket, "431 Request Header Fields Too Large", "").await;
        }
        bytes.push(
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read_u8()).await??,
        );
    }
    let head = String::from_utf8_lossy(&bytes);
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if !matches!(method, "GET" | "HEAD") {
        return error(
            &mut socket,
            "405 Method Not Allowed",
            "Allow: GET, HEAD\r\n",
        )
        .await;
    }
    let segments: Vec<_> = target.split('/').collect();
    if segments.len() != 4
        || !segments[0].is_empty()
        || segments[1] != token
        || !segments[2].starts_with("t3_")
        || !segments[2]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || !segments[3]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || segments[3].starts_with('.')
    {
        return error(&mut socket, "404 Not Found", "").await;
    }
    let directory = root.join(segments[2]);
    let manifest = match tokio::fs::read(directory.join("complete.json")).await {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes).ok(),
        Err(_) => None,
    };
    if !manifest
        .and_then(|manifest| {
            manifest
                .get("files")
                .and_then(|v| v.as_array())
                .map(|files| files.iter().any(|v| v.as_str() == Some(segments[3])))
        })
        .unwrap_or(false)
    {
        return error(&mut socket, "404 Not Found", "").await;
    }
    let path = directory.join(segments[3]);
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return error(&mut socket, "404 Not Found", "").await;
    };
    let length = file.metadata().await?.len();
    let range_header = lines.find_map(|line| {
        line.split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("range"))
            .map(|(_, value)| value.trim())
    });
    let Some((start, end, partial)) = range(range_header, length) else {
        crate::diagnostics::event(
            "playback.range_rejected",
            serde_json::json!({"thread_id":segments[2],"length":length}),
        );
        return error(
            &mut socket,
            "416 Range Not Satisfiable",
            &format!("Content-Range: bytes */{length}\r\n"),
        )
        .await;
    };
    let mime = match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
    {
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    };
    let status = if partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    crate::diagnostics::event(
        "playback.response",
        serde_json::json!({"thread_id":segments[2],"status":if partial{206}else{200},"start":start,"end":end,"file_bytes":length,"mime":mime,"head":method=="HEAD"}),
    );
    let extra = if partial {
        format!("Content-Range: bytes {start}-{end}/{length}\r\n")
    } else {
        String::new()
    };
    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n{extra}\r\n",end-start+1).as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let started = std::time::Instant::now();
    let sent = tokio::io::copy(&mut file.take(end - start + 1), &mut socket).await;
    crate::diagnostics::event(
        "playback.transfer",
        serde_json::json!({"thread_id":segments[2],"bytes_sent":sent.as_ref().ok(),"elapsed_ms":started.elapsed().as_millis() as u64,"error":sent.as_ref().err().map(|e|e.to_string())}),
    );
    sent?;
    Ok(())
}
