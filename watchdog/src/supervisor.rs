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

//! The occupant warning when the Guardian fails (HARA DFR-5).
//!
//! [`Supervisor`] turns heartbeat transitions into `SupervisorEvent`s: a lost
//! heartbeat requests `DRIVER_WARNING_MONITORING_UNAVAILABLE`, its return
//! withdraws it. The events go out on the watchdog's own topic, so the
//! warning depends neither on the Guardian nor on the Evidence Collector.
//! Like [`crate::HeartbeatMonitor`], it does no IO and owns no clock.

use thermal_contract::v1 as pb;

use crate::diagnostics::GuardianSeen;

/// Builds the watchdog's `SupervisorEvent`s and links them by cause.
pub struct Supervisor {
    session_id: String,
    next_event_id: u64,
    /// The `GuardianLost` event of the current outage, if any.
    lost: Option<u64>,
}

impl Supervisor {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            next_event_id: 1,
            lost: None,
        }
    }

    /// The heartbeat was lost: reports the loss and requests the occupant
    /// warning, caused by the loss. Nothing while an outage is already open.
    pub fn on_lost(
        &mut self,
        last: Option<&GuardianSeen>,
        silence_ms: u64,
        now_ms: u64,
    ) -> Vec<pb::SupervisorEvent> {
        use pb::supervisor_event::Kind;
        if self.lost.is_some() {
            return Vec::new();
        }
        let lost = self.event(
            0,
            now_ms,
            Kind::GuardianLost(pb::GuardianLost {
                last_guardian_session_id: last.map(|g| g.session_id.clone()).unwrap_or_default(),
                last_heartbeat_sequence: last.map_or(0, |g| g.sequence),
                silence_ms,
            }),
        );
        self.lost = Some(lost.event_id);
        let warning = self.event(
            lost.event_id,
            now_ms,
            Kind::MitigationRequested(pb::MitigationRequested {
                mitigation: pb::Mitigation::DriverWarningMonitoringUnavailable as i32,
            }),
        );
        vec![lost, warning]
    }

    /// The first heartbeat after a loss: the Guardian is back and its own
    /// warnings apply again. Nothing if no outage is open.
    pub fn on_restored(
        &mut self,
        guardian: &GuardianSeen,
        now_ms: u64,
    ) -> Vec<pb::SupervisorEvent> {
        let Some(lost) = self.lost.take() else {
            return Vec::new();
        };
        vec![self.event(
            lost,
            now_ms,
            pb::supervisor_event::Kind::GuardianRestored(pb::GuardianRestored {
                guardian_session_id: guardian.session_id.clone(),
                heartbeat_sequence: guardian.sequence,
            }),
        )]
    }

    fn event(
        &mut self,
        cause_event_id: u64,
        now_ms: u64,
        kind: pb::supervisor_event::Kind,
    ) -> pb::SupervisorEvent {
        let event_id = self.next_event_id;
        self.next_event_id += 1;
        pb::SupervisorEvent {
            session_id: self.session_id.clone(),
            event_id,
            cause_event_id,
            watchdog_time_ms: now_ms,
            kind: Some(kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb::supervisor_event::Kind;

    fn guardian(sequence: u64) -> GuardianSeen {
        GuardianSeen {
            session_id: "guardian-1".into(),
            sequence,
        }
    }

    #[test]
    fn loss_requests_the_monitoring_unavailable_warning() {
        let mut supervisor = Supervisor::new("watchdog-1");

        let events = supervisor.on_lost(Some(&guardian(7)), 1_600, 5_000);

        assert_eq!(events.len(), 2);
        let (lost, warning) = (&events[0], &events[1]);
        assert!(matches!(
            &lost.kind,
            Some(Kind::GuardianLost(l)) if l.last_guardian_session_id == "guardian-1"
                && l.last_heartbeat_sequence == 7
                && l.silence_ms == 1_600
        ));
        assert_eq!(
            warning.kind,
            Some(Kind::MitigationRequested(pb::MitigationRequested {
                mitigation: pb::Mitigation::DriverWarningMonitoringUnavailable as i32,
            }))
        );
        assert_eq!(warning.cause_event_id, lost.event_id);
        assert_eq!(lost.cause_event_id, 0);
        assert!(events
            .iter()
            .all(|e| e.session_id == "watchdog-1" && e.watchdog_time_ms == 5_000));
    }

    #[test]
    fn a_guardian_that_never_started_is_reported_without_a_last_heartbeat() {
        let mut supervisor = Supervisor::new("watchdog-1");

        let events = supervisor.on_lost(None, 1_501, 1_501);

        assert!(matches!(
            &events[0].kind,
            Some(Kind::GuardianLost(l)) if l.last_guardian_session_id.is_empty()
                && l.last_heartbeat_sequence == 0
        ));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn restoration_is_linked_to_the_loss() {
        let mut supervisor = Supervisor::new("watchdog-1");
        let lost = supervisor.on_lost(Some(&guardian(7)), 1_600, 5_000)[0].event_id;

        let events = supervisor.on_restored(&guardian(8), 9_000);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cause_event_id, lost);
        assert!(matches!(
            &events[0].kind,
            Some(Kind::GuardianRestored(r)) if r.heartbeat_sequence == 8
        ));
    }

    #[test]
    fn one_warning_per_outage() {
        let mut supervisor = Supervisor::new("watchdog-1");
        supervisor.on_lost(None, 1_501, 1_501);

        assert!(supervisor.on_lost(None, 3_000, 3_000).is_empty());
        assert_eq!(supervisor.on_restored(&guardian(1), 4_000).len(), 1);
        assert!(supervisor.on_restored(&guardian(2), 4_500).is_empty());
        assert_eq!(
            supervisor.on_lost(Some(&guardian(2)), 1_600, 7_000).len(),
            2
        );
    }

    #[test]
    fn event_ids_increase_across_outages() {
        let mut supervisor = Supervisor::new("watchdog-1");
        let mut ids = Vec::new();
        ids.extend(supervisor.on_lost(None, 1_501, 1_501));
        ids.extend(supervisor.on_restored(&guardian(1), 2_000));
        ids.extend(supervisor.on_lost(Some(&guardian(1)), 1_600, 4_000));

        let ids: Vec<u64> = ids.iter().map(|e| e.event_id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }
}
