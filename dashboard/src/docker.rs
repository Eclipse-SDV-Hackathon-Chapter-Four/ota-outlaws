// Copyright (c) 2026 Contributors to the Eclipse Foundation
//
// See the NOTICE file(s) distributed with this work for additional
// information regarding copyright ownership.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// https://www.eclipse.org/legal/epl-2.0
//
// SPDX-License-Identifier: EPL-2.0

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5)

//! A minimal client for the Docker Engine API over its Unix socket.
//!
//! It sends HTTP/1.0 requests, so the daemon answers with a plain body and
//! closes the connection. Only the few calls the dashboard needs are here;
//! responses stay `serde_json::Value`, since the dashboard only reads a few
//! fields of each.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context as _};
use serde_json::Value;

/// Stopping a container waits up to 10 s for it to exit.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Docker {
    socket: PathBuf,
}

impl Docker {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Docker {
            socket: socket.into(),
        }
    }

    async fn request(&self, method: &str, path: &str) -> anyhow::Result<(u16, Vec<u8>)> {
        self.request_with_body(method, path, None).await
    }

    async fn request_with_body(
        &self,
        method: &str,
        path: &str,
        json: Option<&Value>,
    ) -> anyhow::Result<(u16, Vec<u8>)> {
        let body = json.map(Value::to_string).unwrap_or_default();
        let content_type = if json.is_some() {
            "Content-Type: application/json\r\n"
        } else {
            ""
        };
        let request = format!(
            "{method} {path} HTTP/1.0\r\nHost: docker\r\n{content_type}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let raw = tokio::time::timeout(REQUEST_TIMEOUT, send(&self.socket, request.as_bytes()))
            .await
            .with_context(|| format!("Docker API {method} {path} timed out"))??;
        parse_response(&raw)
    }

    /// A request that only needs to succeed: 2xx, or 304 (already in that
    /// state), or 404 when `missing_ok`.
    async fn expect_ok(
        &self,
        method: &str,
        path: &str,
        json: Option<&Value>,
        missing_ok: bool,
    ) -> anyhow::Result<Vec<u8>> {
        let (status, body) = self.request_with_body(method, path, json).await?;
        if (200..300).contains(&status) || status == 304 || (missing_ok && status == 404) {
            Ok(body)
        } else {
            bail!("{status} {}", error_message(&body))
        }
    }

    /// Creates a container from a Docker API create body; returns its ID.
    pub async fn create(&self, name: &str, config: &Value) -> anyhow::Result<String> {
        let body = self
            .expect_ok(
                "POST",
                &format!("/containers/create?name={}", encode(name)),
                Some(config),
                false,
            )
            .await?;
        let created: Value = serde_json::from_slice(&body)?;
        created["Id"]
            .as_str()
            .map(str::to_owned)
            .context("no container ID in the create response")
    }

    /// Removes a container and its anonymous volumes; a missing one is fine.
    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        self.expect_ok(
            "DELETE",
            &format!("/containers/{}?force=1&v=1", encode(id)),
            None,
            true,
        )
        .await
        .map(drop)
    }

    /// Networks or volumes (`kind`) that carry `label` (`key=value` or `key`).
    pub async fn labeled(&self, kind: &str, label: &str) -> anyhow::Result<Vec<Value>> {
        let filters = serde_json::json!({ "label": [label] }).to_string();
        let listed = self
            .get_json(&format!("/{kind}?filters={}", encode(&filters)))
            .await?;
        // Volumes come wrapped in {"Volumes": [...]}, networks as a list.
        Ok(match listed {
            Value::Array(items) => items,
            mut other => other["Volumes"]
                .take()
                .as_array()
                .cloned()
                .unwrap_or_default(),
        })
    }

    /// Removes a network or volume (`kind`) by ID or name; a missing one is fine.
    pub async fn remove_object(&self, kind: &str, id: &str) -> anyhow::Result<()> {
        self.expect_ok("DELETE", &format!("/{kind}/{}", encode(id)), None, true)
            .await
            .map(drop)
    }

    async fn get(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let (status, body) = self.request("GET", path).await?;
        if !(200..300).contains(&status) {
            bail!("Docker API GET {path}: {status} {}", error_message(&body));
        }
        Ok(body)
    }

    async fn get_json(&self, path: &str) -> anyhow::Result<Value> {
        let body = self.get(path).await?;
        serde_json::from_slice(&body).with_context(|| format!("Docker API GET {path}: not JSON"))
    }

    async fn post(&self, path: &str) -> anyhow::Result<()> {
        let (status, body) = self.request("POST", path).await?;
        // 304: already started or already stopped.
        if !(200..300).contains(&status) && status != 304 {
            bail!("{status} {}", error_message(&body));
        }
        Ok(())
    }

    /// All containers, running or not, that carry `label` (`key=value`).
    pub async fn containers(&self, label: &str) -> anyhow::Result<Vec<Value>> {
        let filters = serde_json::json!({ "label": [label] }).to_string();
        let path = format!("/containers/json?all=1&filters={}", encode(&filters));
        match self.get_json(&path).await? {
            Value::Array(items) => Ok(items),
            other => bail!("unexpected container list: {other}"),
        }
    }

    pub async fn inspect(&self, id: &str) -> anyhow::Result<Value> {
        self.get_json(&format!("/containers/{}/json", encode(id)))
            .await
    }

    /// One sample of the container's resource usage.
    pub async fn stats(&self, id: &str) -> anyhow::Result<Value> {
        self.get_json(&format!(
            "/containers/{}/stats?stream=false&one-shot=true",
            encode(id)
        ))
        .await
    }

    pub async fn start(&self, id: &str) -> anyhow::Result<()> {
        self.post(&format!("/containers/{}/start", encode(id)))
            .await
    }

    pub async fn stop(&self, id: &str) -> anyhow::Result<()> {
        self.post(&format!("/containers/{}/stop?t=10", encode(id)))
            .await
    }

    pub async fn restart(&self, id: &str) -> anyhow::Result<()> {
        self.post(&format!("/containers/{}/restart?t=10", encode(id)))
            .await
    }

    /// The last `tail` lines of stdout and stderr, each starting with its
    /// RFC 3339 timestamp.
    pub async fn logs(&self, id: &str, tail: usize) -> anyhow::Result<Vec<String>> {
        let body = self
            .get(&format!(
                "/containers/{}/logs?stdout=1&stderr=1&timestamps=1&tail={tail}",
                encode(id)
            ))
            .await?;
        let text = String::from_utf8_lossy(&demux(&body)).into_owned();
        Ok(text.lines().map(str::to_owned).collect())
    }

    /// The content of a file inside the container.
    pub async fn file(&self, id: &str, path: &str) -> anyhow::Result<String> {
        let body = self
            .get(&format!(
                "/containers/{}/archive?path={}",
                encode(id),
                encode(path)
            ))
            .await?;
        let content = first_file_in_tar(&body).context("no regular file in the archive")?;
        Ok(String::from_utf8_lossy(content).into_owned())
    }
}

#[cfg(unix)]
async fn send(socket: &std::path::Path, request: &[u8]) -> anyhow::Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .with_context(|| format!("cannot connect to the Docker socket {}", socket.display()))?;
    stream.write_all(request).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(response)
}

#[cfg(not(unix))]
async fn send(socket: &std::path::Path, _request: &[u8]) -> anyhow::Result<Vec<u8>> {
    bail!(
        "the Docker socket {} is only supported on Unix; run the dashboard in its container",
        socket.display()
    )
}

/// Splits an HTTP response into status code and body.
fn parse_response(raw: &[u8]) -> anyhow::Result<(u16, Vec<u8>)> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("incomplete HTTP response from Docker")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .context("no HTTP status line from Docker")?;
    let chunked = head.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });
    Ok((
        status,
        if chunked {
            dechunk(body)
        } else {
            body.to_vec()
        },
    ))
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = body.windows(2).position(|w| w == b"\r\n") {
        let size_field = String::from_utf8_lossy(&body[..end]);
        let size_hex = size_field.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        let start = end + 2;
        if size == 0 || start + size > body.len() {
            break;
        }
        out.extend_from_slice(&body[start..start + size]);
        body = &body[(start + size + 2).min(body.len())..];
    }
    out
}

fn error_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_owned())
}

/// Removes the 8-byte frame headers Docker puts in front of every chunk of a
/// log stream when the container has no TTY.
fn demux(body: &[u8]) -> Vec<u8> {
    let multiplexed = body.len() >= 8 && body[0] <= 2 && body[1..4] == [0, 0, 0];
    if !multiplexed {
        return body.to_vec();
    }
    let mut out = Vec::with_capacity(body.len());
    let mut rest = body;
    while rest.len() >= 8 {
        let size = u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        let end = (8 + size).min(rest.len());
        out.extend_from_slice(&rest[8..end]);
        rest = &rest[end..];
    }
    out
}

/// The content of the first regular file in a tar archive.
fn first_file_in_tar(tar: &[u8]) -> Option<&[u8]> {
    let mut offset = 0;
    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        if header.iter().all(|&b| b == 0) {
            return None;
        }
        let size_field = String::from_utf8_lossy(&header[124..136]);
        let size = usize::from_str_radix(size_field.trim_matches(['\0', ' ']), 8).ok()?;
        let start = offset + 512;
        if matches!(header[156], b'0' | 0) {
            return tar.get(start..start + size);
        }
        offset = start + size.div_ceil(512) * 512;
    }
    None
}

/// Percent-encodes everything but unreserved characters.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_body() {
        let raw = b"HTTP/1.0 404 Not Found\r\nContent-Type: application/json\r\n\r\n{\"message\":\"no such container\"}";
        let (status, body) = parse_response(raw).unwrap();
        assert_eq!(status, 404);
        assert_eq!(error_message(&body), "no such container");
    }

    #[test]
    fn decodes_a_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap().1, b"Wikipedia");
    }

    #[test]
    fn removes_log_frame_headers() {
        let mut body = vec![1, 0, 0, 0, 0, 0, 0, 6];
        body.extend_from_slice(b"hello\n");
        body.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 4]);
        body.extend_from_slice(b"err\n");
        assert_eq!(demux(&body), b"hello\nerr\n");
        assert_eq!(demux(b"plain tty output\n"), b"plain tty output\n");
    }

    #[test]
    fn reads_the_first_file_of_a_tar() {
        let mut header = [0u8; 512];
        header[..9].copy_from_slice(b"params.tx");
        header[124..135].copy_from_slice(b"00000000005");
        header[156] = b'0';
        let mut tar = header.to_vec();
        let mut data = [0u8; 512];
        data[..5].copy_from_slice(b"a = 1");
        tar.extend_from_slice(&data);
        tar.extend_from_slice(&[0u8; 1024]);
        assert_eq!(first_file_in_tar(&tar), Some(&b"a = 1"[..]));
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("{\"a\":[1]}"), "%7B%22a%22%3A%5B1%5D%7D");
        assert_eq!(encode("/etc/x.toml"), "%2Fetc%2Fx.toml");
    }
}
