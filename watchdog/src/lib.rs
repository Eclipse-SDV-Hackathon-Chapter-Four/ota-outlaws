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

//! Heartbeat supervision of the Battery Thermal Guardian (FSR-2.7).
//!
//! [`HeartbeatMonitor`] decides whether the Guardian is alive. It does no IO
//! and owns no clock: the caller passes the time, so the logic is tested
//! without a network or DFM. The executable in `main.rs` connects it to
//! uProtocol and DFM.

pub mod diagnostics;

/// Whether the Guardian's heartbeat is currently present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogState {
    /// A heartbeat arrived within the timeout, or the timeout has not yet
    /// passed since the watchdog started.
    Healthy,
    /// No heartbeat for longer than the timeout: the Guardian crashed, hung,
    /// or never started.
    Lost,
}

/// A change worth reporting to DFM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    None,
    /// The first heartbeat after the watchdog started: the first healthy
    /// test. Reported as `Passed`, which also clears a failure left in DFM
    /// by an earlier session, as the Guardian does with `FaultTestPassed`.
    FirstHeartbeat,
    BecameLost,
    Recovered,
}

/// Tracks whether a heartbeat has been seen within `T_hb`.
pub struct HeartbeatMonitor {
    timeout_ms: u64,
    last_seen_ms: Option<u64>,
    state: WatchdogState,
}

impl HeartbeatMonitor {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            last_seen_ms: None,
            state: WatchdogState::Healthy,
        }
    }

    pub fn state(&self) -> WatchdogState {
        self.state
    }

    /// How long no heartbeat has arrived, in milliseconds. Before the first
    /// heartbeat, counted from the watchdog's start (time 0).
    pub fn silence_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_seen_ms.unwrap_or(0))
    }

    /// Call when a heartbeat arrives.
    pub fn on_heartbeat(&mut self, now_ms: u64) -> Transition {
        let first = self.last_seen_ms.is_none();
        self.last_seen_ms = Some(now_ms);
        if self.state == WatchdogState::Lost {
            self.state = WatchdogState::Healthy;
            Transition::Recovered
        } else if first {
            Transition::FirstHeartbeat
        } else {
            Transition::None
        }
    }

    /// Call periodically. Reports `BecameLost` once per outage, not on every
    /// call while the Guardian stays silent.
    pub fn on_tick(&mut self, now_ms: u64) -> Transition {
        if self.state == WatchdogState::Healthy && self.silence_ms(now_ms) > self.timeout_ms {
            self.state = WatchdogState::Lost;
            Transition::BecameLost
        } else {
            Transition::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T_HB: u64 = 1_500;

    #[test]
    fn stays_healthy_until_the_timeout_after_start() {
        let mut monitor = HeartbeatMonitor::new(T_HB);

        assert_eq!(monitor.on_tick(T_HB), Transition::None);
        assert_eq!(monitor.state(), WatchdogState::Healthy);
    }

    #[test]
    fn a_guardian_that_never_starts_is_reported() {
        let mut monitor = HeartbeatMonitor::new(T_HB);

        assert_eq!(monitor.on_tick(T_HB + 1), Transition::BecameLost);
        assert_eq!(monitor.state(), WatchdogState::Lost);
    }

    #[test]
    fn the_first_heartbeat_is_the_first_healthy_test() {
        let mut monitor = HeartbeatMonitor::new(T_HB);

        assert_eq!(monitor.on_heartbeat(100), Transition::FirstHeartbeat);
        assert_eq!(monitor.on_heartbeat(600), Transition::None);
    }

    #[test]
    fn regular_heartbeats_keep_it_healthy() {
        let mut monitor = HeartbeatMonitor::new(T_HB);
        monitor.on_heartbeat(0);

        for now in (500..10_000).step_by(500) {
            assert_eq!(monitor.on_heartbeat(now), Transition::None);
            assert_eq!(monitor.on_tick(now + 400), Transition::None);
        }
    }

    #[test]
    fn a_guardian_that_starts_late_recovers_on_its_first_heartbeat() {
        let mut monitor = HeartbeatMonitor::new(T_HB);
        monitor.on_tick(T_HB + 1);

        assert_eq!(monitor.on_heartbeat(3_000), Transition::Recovered);
    }

    #[test]
    fn a_gap_longer_than_the_timeout_is_reported_once() {
        let mut monitor = HeartbeatMonitor::new(T_HB);
        monitor.on_heartbeat(1_000);

        assert_eq!(monitor.on_tick(1_000 + T_HB), Transition::None);
        assert_eq!(monitor.on_tick(1_000 + T_HB + 1), Transition::BecameLost);
        assert_eq!(monitor.on_tick(10_000), Transition::None);
        assert_eq!(monitor.silence_ms(10_000), 9_000);
    }

    #[test]
    fn the_next_heartbeat_after_an_outage_recovers() {
        let mut monitor = HeartbeatMonitor::new(T_HB);
        monitor.on_heartbeat(0);
        monitor.on_tick(T_HB + 1);

        assert_eq!(monitor.on_heartbeat(5_000), Transition::Recovered);
        assert_eq!(monitor.state(), WatchdogState::Healthy);
        assert_eq!(monitor.on_tick(5_000 + T_HB + 1), Transition::BecameLost);
    }
}
