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

//! Domain types of the Guardian core.
//!
//! These types are independent of any transport. Adapters translate uProtocol
//! messages into [`Sample`]s and Guardian [`Event`]s into uProtocol messages and
//! DFM records.

/// Point in time on the Guardian's local monotonic clock, in milliseconds.
///
/// The core never reads a clock itself. The caller passes the current time into
/// every call, which makes the core deterministic and replayable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Millis(pub u64);

impl Millis {
    /// Milliseconds elapsed since `earlier`, or zero if `earlier` is later.
    pub fn since(self, earlier: Millis) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

/// One battery temperature sample, as delivered by the VSS Publisher.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Time the values were captured at their origin, on the source's clock
    /// (assumption A-1). Only compared with other source timestamps, never with
    /// the Guardian's clock.
    pub source_timestamp_ms: u64,
    /// Sequence number set by the VSS Publisher, increasing by one per message (A-1).
    /// The Guardian does not evaluate it; the Evidence Collector does (EC-2).
    pub sequence: u64,
    /// Alive counter of the CAN frame, incremented by the source per frame and
    /// wrapping from 255 to 0 (A-3).
    pub alive_counter: u8,
    /// Quality flag of the CAN frame (A-3).
    pub quality: Quality,
    /// Maximum cell temperature in °C.
    pub max_c: f32,
    /// Average cell temperature in °C.
    pub avg_c: f32,
    /// Minimum cell temperature in °C.
    pub min_c: f32,
}

impl Sample {
    pub(crate) fn reference(&self) -> SampleRef {
        SampleRef {
            sequence: self.sequence,
            source_timestamp_ms: self.source_timestamp_ms,
            alive_counter: self.alive_counter,
        }
    }

    pub(crate) fn has_finite_values(&self) -> bool {
        self.max_c.is_finite() && self.avg_c.is_finite() && self.min_c.is_finite()
    }
}

/// Identifies the sample that triggered an event, for the evidence chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleRef {
    pub sequence: u64,
    pub source_timestamp_ms: u64,
    pub alive_counter: u8,
}

/// Quality flag the source sets in the CAN frame (`Quality Enum` in
/// `docs/reference/architecture.md`). Only `Valid` samples are evaluated
/// (FSR-3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quality {
    Valid,
    Invalid,
    /// The source reports an error or no value, or the flag is unknown.
    NotAvailable,
}

/// How dangerous the battery temperature is.
///
/// See "Guardian output model" in `docs/explanation/safety-concept.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThermalState {
    /// Initial state: no valid data has been received yet. Not "all clear".
    Clear,
    Monitoring,
    Warning,
    Critical,
    /// Same severity as `Critical`; the mitigation has been requested.
    /// Not reachable yet: belongs to FSR-1.6.
    Mitigating,
}

impl ThermalState {
    /// Severity rank. `Critical` and `Mitigating` rank equally.
    pub fn severity(self) -> u8 {
        match self {
            ThermalState::Clear => 0,
            ThermalState::Monitoring => 1,
            ThermalState::Warning => 2,
            ThermalState::Critical | ThermalState::Mitigating => 3,
        }
    }
}

/// Whether the Guardian can currently trust its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonitoringStatus {
    Ok,
    /// The source may be frozen: repeated frames arrived (FSR-2.3). Isolated
    /// invalid samples (FSR-3.5) will also lead here once implemented.
    Suspect,
    Degraded,
}

/// A fault the Guardian can detect.
///
/// Each fault code identifies the requirement that detects it (FSR-D.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultCode {
    /// No fresh sample for longer than the freshness timeout.
    FreshnessLost,
    /// Samples keep arriving, but the alive counter does not change.
    CounterStuck,
    /// Maximum temperature frozen while other temperatures change.
    SignalStuck,
    /// The source marked the sample as not usable.
    QualityInvalid,
    /// Minimum, average, and maximum are not in order.
    OrderImplausible,
    /// A temperature is outside the plausible range.
    OutOfRange,
    /// The maximum rose faster than physically plausible.
    RateImplausible,
    /// No fresh sample since the Guardian started.
    NoDataAtStartup,
}

impl FaultCode {
    /// Complete diagnostic fault set emitted by this core.
    pub const ALL: [Self; 8] = [
        Self::FreshnessLost,
        Self::CounterStuck,
        Self::SignalStuck,
        Self::QualityInvalid,
        Self::OrderImplausible,
        Self::OutOfRange,
        Self::RateImplausible,
        Self::NoDataAtStartup,
    ];

    /// The functional safety requirement that detects this fault.
    pub fn requirement(self) -> &'static str {
        match self {
            FaultCode::FreshnessLost => "FSR-2.2",
            FaultCode::CounterStuck => "FSR-2.3",
            FaultCode::SignalStuck => "FSR-2.4",
            FaultCode::QualityInvalid => "FSR-3.4",
            FaultCode::OrderImplausible => "FSR-3.1",
            FaultCode::OutOfRange => "FSR-3.2",
            FaultCode::RateImplausible => "FSR-3.3",
            FaultCode::NoDataAtStartup => "FSR-2.1",
        }
    }

    /// Diagnostic trouble code used in the DFM fault catalog.
    pub fn dtc(self) -> &'static str {
        match self {
            FaultCode::FreshnessLost => "BTG_TempFreshnessLost",
            FaultCode::CounterStuck => "BTG_TempCounterStuck",
            FaultCode::SignalStuck => "BTG_TempSignalStuck",
            FaultCode::QualityInvalid => "BTG_TempQualityInvalid",
            FaultCode::OrderImplausible => "BTG_TempOrderImplausible",
            FaultCode::OutOfRange => "BTG_TempOutOfRange",
            FaultCode::RateImplausible => "BTG_TempRateImplausible",
            FaultCode::NoDataAtStartup => "BTG_TempNoDataAtStartup",
        }
    }
}

/// Mitigation the Guardian requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mitigation {
    /// The battery is critically hot.
    DriverWarningOvertemp,
    /// Thermal monitoring is unavailable.
    DriverWarningMonitoringUnavailable,
}

/// Identifies an event within one Guardian run. Assigned in order, starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub u64);

/// Something the Guardian reports. Adapters publish events over uProtocol and
/// write fault events to the DFM.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: EventId,
    /// The event that caused this one, if any. Links the evidence chain
    /// fault → state change → mitigation.
    pub cause: Option<EventId>,
    /// Local time at which the Guardian produced the event.
    pub at: Millis,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    FaultDetected {
        fault: FaultCode,
        /// The last fresh sample before the fault was detected, if any.
        last_sample: Option<SampleRef>,
    },
    /// A monitor completed its first sustained healthy test in this session.
    FaultTestPassed {
        fault: FaultCode,
        trigger: SampleRef,
    },
    FaultRecovered {
        fault: FaultCode,
        trigger: SampleRef,
    },
    ThermalStateChanged {
        from: ThermalState,
        to: ThermalState,
        /// The sample that caused the change.
        trigger: SampleRef,
    },
    MonitoringStatusChanged {
        from: MonitoringStatus,
        to: MonitoringStatus,
    },
    MitigationRequested {
        mitigation: Mitigation,
    },
}
