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

//! Generates the KUKSA Data Broker client. Uses a vendored `protoc` and its
//! bundled well-known types, so no system installation is needed.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // tonic-build 0.11 finds protoc through this variable.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    let well_known_types = protoc_bin_vendored::include_path()?;

    tonic_build::configure().build_server(false).compile(
        &["proto/kuksa/val/v1/val.proto"],
        &[std::path::PathBuf::from("proto"), well_known_types],
    )?;
    Ok(())
}
