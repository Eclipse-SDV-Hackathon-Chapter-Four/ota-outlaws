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

// AI-assisted: Claude Code / Claude Opus 5.5 (claude-opus-5-5); Codex / GPT-6.1 Sol (gpt-6.1-sol)

//! Adapters around the Guardian core: uProtocol input, tick, and uProtocol
//! output. The design is described in
//! `docs/reference/components/battery-thermal-guardian.md`.

pub mod convert;
pub mod runtime;
pub mod transport;

pub mod diagnostics;
