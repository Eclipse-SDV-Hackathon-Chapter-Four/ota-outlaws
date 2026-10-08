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
// AI-assisted: Codex / GPT-6 (gpt-6)

//! HTTP adapter to the host's AutoSD/openDuT bench owner.
use anyhow::{bail, Context as _};
use serde_json::{json, Value};
use std::time::Duration;

pub struct Remote {
    url: String,
    client: reqwest::Client,
}

impl Remote {
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Ok(url) = std::env::var("OPENDUT_CONTROLLER_URL") else {
            return Ok(None);
        };
        let parsed = reqwest::Url::parse(&url).context("invalid OPENDUT_CONTROLLER_URL")?;
        if parsed.scheme() != "http" || !parsed.username().is_empty() || parsed.password().is_some()
        {
            bail!("OPENDUT_CONTROLLER_URL must be an HTTP URL to the local host service");
        }
        Ok(Some(Self {
            url: url.trim_end_matches('/').into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()?,
        }))
    }

    pub async fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.request(path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        self.request(path, Some(body)).await
    }

    async fn request(&self, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let url = format!("{}{path}", self.url);
        let request = if let Some(body) = body {
            self.client.post(url).json(&body)
        } else {
            self.client.get(url)
        };
        let response = request
            .header("X-Dashboard", "1")
            .send()
            .await
            .context("OpenDUT host controller unreachable")?;
        let status = response.status();
        let value: Value = response.json().await?;
        if !status.is_success() {
            bail!(
                "{}",
                value["error"]
                    .as_str()
                    .unwrap_or("OpenDUT controller request failed")
            );
        }
        Ok(value)
    }

    pub async fn status(&self) -> Value {
        self.get("/status").await.unwrap_or_else(|error| {
            json!({
                "backend": "opendut", "available": false, "unavailable": format!("{error:#}"),
                "running": false, "exists": false,
            })
        })
    }
}
