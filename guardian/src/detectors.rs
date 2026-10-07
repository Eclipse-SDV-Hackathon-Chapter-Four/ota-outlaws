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

//! Input monitoring detectors. Each detector implements one requirement and
//! only answers whether its fault condition holds; the Guardian decides what
//! happens next.

use crate::config::{FreshnessConfig, StuckConfig};
use crate::model::{Millis, Sample};

/// Repeated frames since the last fresh sample from which the source counts as
/// repeating itself (FSR-2.3). A single repeated frame, such as a duplicate,
/// is not enough.
const MIN_REPEATED_FRAMES: u32 = 2;

/// FSR-2.2: detects that no fresh sample arrived for longer than `T_stale`.
/// FSR-2.3: tells whether the source kept sending the same frame meanwhile.
#[derive(Debug, Clone)]
pub(crate) struct FreshnessMonitor {
    stale_timeout_ms: u64,
    last_fresh_at: Option<Millis>,
    repeated_frames: u32,
}

impl FreshnessMonitor {
    pub(crate) fn new(config: &FreshnessConfig) -> Self {
        Self {
            stale_timeout_ms: config.stale_timeout_ms,
            last_fresh_at: None,
            repeated_frames: 0,
        }
    }

    pub(crate) fn record_fresh_sample(&mut self, now: Millis) {
        self.last_fresh_at = Some(now);
        self.repeated_frames = 0;
    }

    pub(crate) fn stale_timeout_ms(&self) -> u64 {
        self.stale_timeout_ms
    }

    /// Records a newer frame that carries the alive counter of the last fresh
    /// sample.
    pub(crate) fn record_repeated_frame(&mut self) {
        self.repeated_frames = self.repeated_frames.saturating_add(1);
    }

    /// True if the source keeps sending, but repeats the same frame.
    pub(crate) fn source_repeats_itself(&self) -> bool {
        self.repeated_frames >= MIN_REPEATED_FRAMES
    }

    /// True once the last fresh sample is older than `T_stale`. Before the first
    /// fresh sample, the startup case applies instead (FSR-2.1, not implemented).
    pub(crate) fn is_stale(&self, now: Millis) -> bool {
        self.last_fresh_at
            .is_some_and(|last| now.since(last) > self.stale_timeout_ms)
    }
}

/// FSR-2.4: detects a maximum temperature that stays frozen while the average or
/// minimum temperature moves by at least `Δ_stuck`.
///
/// Requiring the other signals to move avoids false alarms for a battery at a
/// constant temperature.
#[derive(Debug, Clone)]
pub(crate) struct StuckDetector {
    timeout_ms: u64,
    min_reference_change_c: f32,
    reference: Option<StuckReference>,
}

#[derive(Debug, Clone, Copy)]
struct StuckReference {
    max_c: f32,
    avg_c: f32,
    min_c: f32,
    since: Millis,
}

impl StuckDetector {
    pub(crate) fn new(config: &StuckConfig) -> Self {
        Self {
            timeout_ms: config.timeout_ms,
            min_reference_change_c: config.min_reference_change_c,
            reference: None,
        }
    }

    /// Observes a fresh sample. Returns true if the maximum is stuck.
    pub(crate) fn observe(&mut self, sample: &Sample, now: Millis) -> bool {
        match self.reference {
            // A stuck value repeats bit for bit, so exact comparison is intended.
            Some(reference) if reference.max_c.to_bits() == sample.max_c.to_bits() => {
                let frozen_long_enough = now.since(reference.since) > self.timeout_ms;
                let others_moved = (sample.avg_c - reference.avg_c).abs()
                    >= self.min_reference_change_c
                    || (sample.min_c - reference.min_c).abs() >= self.min_reference_change_c;
                frozen_long_enough && others_moved
            }
            _ => {
                self.reference = Some(StuckReference {
                    max_c: sample.max_c,
                    avg_c: sample.avg_c,
                    min_c: sample.min_c,
                    since: now,
                });
                false
            }
        }
    }
}
