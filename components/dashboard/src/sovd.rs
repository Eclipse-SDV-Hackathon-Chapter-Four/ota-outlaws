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

//! OpenSOVD client: the fault list of one entity, one fault with its
//! environment data, and clearing faults (`DELETE`, which the gateway
//! supports for the whole list and for a single fault).
//!
//! OpenSOVD reports the severity as a number. The fault catalog that DFM
//! loads has the names, summaries, categories, and severities, so every fault
//! is joined with its catalog entry.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context as _};
use serde::Serialize;
use serde_json::Value;

/// `fault_lib::FaultSeverity` in declaration order; OpenSOVD reports its index.
const SEVERITIES: [&str; 6] = ["Trace", "Debug", "Info", "Warn", "Error", "Fatal"];

#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    pub name: String,
    pub summary: String,
    pub category: String,
    pub severity: String,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub version: Option<u64>,
    pub entries: BTreeMap<String, CatalogEntry>,
}

impl Catalog {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let json: Value = serde_json::from_str(text)?;
        let Some(faults) = json["faults"].as_array() else {
            bail!("the catalog has no faults list");
        };
        let field = |fault: &Value, name: &str| fault[name].as_str().unwrap_or("").to_owned();
        let entries = faults
            .iter()
            .filter_map(|fault| {
                let code = fault["id"]["Text"].as_str()?.to_owned();
                Some((
                    code,
                    CatalogEntry {
                        name: field(fault, "name"),
                        summary: field(fault, "summary"),
                        category: field(fault, "category"),
                        severity: field(fault, "severity"),
                    },
                ))
            })
            .collect();
        Ok(Catalog {
            version: json["version"].as_u64(),
            entries,
        })
    }
}

pub struct Sovd {
    client: reqwest::Client,
    /// For example `http://opensovd-gateway:7690/sovd/v1`.
    base: String,
    pub entity: String,
    pub catalog: Catalog,
}

impl Sovd {
    pub fn new(base: String, entity: String, catalog: Catalog) -> Self {
        Sovd {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("HTTP client"),
            base: base.trim_end_matches('/').to_owned(),
            entity,
            catalog,
        }
    }

    pub fn faults_url(&self) -> String {
        format!("{}/apps/{}/faults", self.base, self.entity)
    }

    fn fault_url(&self, code: &str) -> String {
        format!("{}/{}", self.faults_url(), crate::docker::encode(code))
    }

    /// The fault list as OpenSOVD returns it.
    pub async fn raw_faults(&self) -> anyhow::Result<Vec<Value>> {
        let response = self.client.get(self.faults_url()).send().await?;
        let status = response.status();
        if !status.is_success() {
            bail!("{} answered {status}", self.faults_url());
        }
        let body: Value = response.json().await?;
        match body["items"].as_array() {
            Some(items) => Ok(items.clone()),
            None => bail!("{} returned no items", self.faults_url()),
        }
    }

    /// The fault list, each fault with its catalog entry and severity name.
    pub async fn faults(&self) -> anyhow::Result<Vec<Value>> {
        Ok(self
            .raw_faults()
            .await?
            .into_iter()
            .map(|item| self.enrich(item))
            .collect())
    }

    pub fn enrich(&self, mut item: Value) -> Value {
        let code = item["code"].as_str().unwrap_or("").to_owned();
        let numeric = item["severity"]
            .as_u64()
            .and_then(|i| SEVERITIES.get(i as usize).copied());
        let catalog = self.catalog.entries.get(&code);
        let severity_name = catalog
            .map(|c| c.severity.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| numeric.map(str::to_owned))
            .unwrap_or_else(|| "Unknown".to_owned());
        item["severity_name"] = Value::String(severity_name);
        item["catalog"] = serde_json::to_value(catalog).unwrap_or(Value::Null);
        item
    }

    /// One fault, with its environment data if DFM has any. Returns the HTTP
    /// status as well, so a 404 reaches the browser as such.
    pub async fn fault(&self, code: &str) -> anyhow::Result<(u16, Value)> {
        let response = self.client.get(self.fault_url(code)).send().await?;
        let status = response.status().as_u16();
        let body = response.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    /// Clears one fault, or all faults of the entity with `None`.
    pub async fn clear(&self, code: Option<&str>) -> anyhow::Result<(u16, String)> {
        let url = match code {
            Some(code) => self.fault_url(code),
            None => self.faults_url(),
        };
        let response = self.client.delete(&url).send().await?;
        let status = response.status().as_u16();
        Ok((status, response.text().await.unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sovd() -> Sovd {
        let catalog = Catalog::parse(
            r#"{"id":"battery_guardian","version":5,"faults":[
                {"id":{"Text":"BTG_TempOverTempCritical"},"name":"BTG_TempOverTempCritical",
                 "summary":"FSR-1.2: critical","category":"Hardware","severity":"Fatal"}]}"#,
        )
        .unwrap();
        Sovd::new(
            "http://gateway:7690/sovd/v1/".into(),
            "battery_guardian".into(),
            catalog,
        )
    }

    #[test]
    fn severity_comes_from_the_catalog() {
        let item =
            sovd().enrich(serde_json::json!({"code": "BTG_TempOverTempCritical", "severity": 5}));
        assert_eq!(item["severity_name"], "Fatal");
        assert_eq!(item["catalog"]["category"], "Hardware");
    }

    #[test]
    fn unknown_faults_fall_back_to_the_numeric_severity() {
        let item = sovd().enrich(serde_json::json!({"code": "OTHER", "severity": 3}));
        assert_eq!(item["severity_name"], "Warn");
        assert!(item["catalog"].is_null());
    }

    #[test]
    fn urls_are_built_from_the_base() {
        let sovd = sovd();
        assert_eq!(
            sovd.faults_url(),
            "http://gateway:7690/sovd/v1/apps/battery_guardian/faults"
        );
        assert_eq!(
            sovd.fault_url("BTG_X"),
            "http://gateway:7690/sovd/v1/apps/battery_guardian/faults/BTG_X"
        );
    }

    #[test]
    fn the_shipped_catalog_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/diagnostics/catalog/battery_guardian.json");
        let catalog = Catalog::load(&path).unwrap();
        assert!(catalog.entries.len() >= 8);
        assert!(catalog
            .entries
            .values()
            .all(|e| SEVERITIES.contains(&e.severity.as_str())));
    }
}
