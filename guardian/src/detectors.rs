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

use std::collections::VecDeque;

use crate::config::{FreshnessConfig, StuckConfig, ThermalConfig};
use crate::model::{Millis, Sample};

/// What a repeated frame means for the source (FSR-2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repetition {
    /// Fewer repeated frames than `N_suspect`, such as a single duplicate.
    Isolated,
    /// At least `N_suspect` repeated frames: the source may be frozen.
    Suspect,
    /// At least `N_stuck` repeated frames: the source repeats itself.
    Stuck,
}

/// FSR-2.2: detects that neither a fresh sample nor a repeated frame arrived
/// for longer than `T_stale`.
/// FSR-2.3: counts the repeated frames since the last fresh sample.
#[derive(Debug, Clone)]
pub(crate) struct FreshnessMonitor {
    stale_timeout_ms: u64,
    suspect_repeated_frames: u32,
    stuck_repeated_frames: u32,
    /// Last fresh sample or repeated frame. Repeated frames show that the
    /// source still sends, so FSR-2.3 judges them instead of FSR-2.2.
    last_frame_at: Option<Millis>,
    repeated_frames: u32,
}

impl FreshnessMonitor {
    pub(crate) fn new(config: &FreshnessConfig) -> Self {
        Self {
            stale_timeout_ms: config.stale_timeout_ms,
            suspect_repeated_frames: config.suspect_repeated_frames,
            stuck_repeated_frames: config.stuck_repeated_frames,
            last_frame_at: None,
            repeated_frames: 0,
        }
    }

    pub(crate) fn record_fresh_sample(&mut self, now: Millis) {
        self.last_frame_at = Some(now);
        self.repeated_frames = 0;
    }

    pub(crate) fn stale_timeout_ms(&self) -> u64 {
        self.stale_timeout_ms
    }

    /// Records a newer frame that carries the alive counter of the last fresh
    /// sample. Only called after a fresh sample.
    pub(crate) fn record_repeated_frame(&mut self, now: Millis) -> Repetition {
        self.last_frame_at = Some(now);
        self.repeated_frames = self.repeated_frames.saturating_add(1);
        if self.repeated_frames >= self.stuck_repeated_frames {
            Repetition::Stuck
        } else if self.repeated_frames >= self.suspect_repeated_frames {
            Repetition::Suspect
        } else {
            Repetition::Isolated
        }
    }

    /// True once the last fresh sample or repeated frame is older than
    /// `T_stale`. Before the first fresh sample, the startup case applies
    /// instead (FSR-2.1).
    pub(crate) fn is_stale(&self, now: Millis) -> bool {
        self.last_frame_at
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

/// FSR-1.3: detects a valid maximum that rises by at least `r_trend` on
/// average over at least `T_trend` (HARA F-7, TS-27).
///
/// Works on source timestamps, so delivery jitter does not distort the rate.
#[derive(Debug, Clone)]
pub(crate) struct TrendDetector {
    rise_c_per_s: f32,
    duration_ms: u64,
    /// Valid maxima with their source timestamps. Keeps exactly one entry at
    /// or before the start of the window, so the window spans `T_trend`.
    history: VecDeque<(u64, f32)>,
}

impl TrendDetector {
    pub(crate) fn new(config: &ThermalConfig) -> Self {
        Self {
            rise_c_per_s: config.trend_rise_c_per_s,
            duration_ms: config.trend_duration_ms,
            history: VecDeque::new(),
        }
    }

    /// Observes a valid sample. Returns true while the rising trend holds.
    pub(crate) fn observe(&mut self, sample: &Sample) -> bool {
        let now = sample.source_timestamp_ms;
        self.history.push_back((now, sample.max_c));
        while self
            .history
            .get(1)
            .is_some_and(|&(time, _)| now.saturating_sub(time) >= self.duration_ms)
        {
            self.history.pop_front();
        }
        let (start, start_max) = self.history[0];
        let span_ms = now.saturating_sub(start);
        // The average rate over the actual span, so that a gap in the data
        // cannot stretch the window and fake a slow rise into a trend.
        span_ms >= self.duration_ms
            && sample.max_c - start_max >= self.rise_c_per_s * span_ms as f32 / 1000.0
    }
}
