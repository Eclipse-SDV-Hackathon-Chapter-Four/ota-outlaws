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

//! Generates the Rust types from `battery_thermal.proto`. Uses a vendored
//! `protoc`, so no system installation is needed.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "battery_thermal.proto";
    println!("cargo:rerun-if-changed={proto}");

    // prost-build 0.12 finds protoc through this variable.
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    prost_build::compile_protos(&[proto], &["."])?;
    Ok(())
}
