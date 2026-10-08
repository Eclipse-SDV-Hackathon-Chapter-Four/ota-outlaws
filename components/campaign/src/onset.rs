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

//! Fault onset detectors. The onset t0 is the first observation of the fault
//! at the Guardian's input (the Campaign Tool onset convention), not the time
//! the tool injected it: a fault that never reaches the Guardian must not count
//! as detected.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::recording::Temperature;

/// How to find the onset in the recorded `BatteryTemperature` stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Onset {
    /// Nominal scenario: the onset is the first sample.
    None,
    /// No sample at all reaches the Guardian's input; the onset is the start
    /// of the recording.
    NoInput,
    /// The stream stops for good; t0 is the last sample plus one cycle.
    StreamEnd,
    /// The tool's own injection with this action, for a fault behind the tap
    /// that the tap cannot observe, such as cutting the Guardian off the
    /// network. The only onset taken from the tool instead of the tap.
    Injection(String),
    /// A gap longer than `T_stale`; t0 is the last sample before it plus one
    /// signal cycle (the first sample that is missing).
    Gap,
    /// A sample with the same alive counter as the one before.
    AliveCounterRepeats,
    /// A sample whose alive counter jumps by 3 or more. Smaller jumps happen
    /// when the chain drops a single frame and are not a source fault.
    AliveCounterJumps,
    /// A sample with a quality other than VALID.
    QualityNotValid,
    /// The first sample of a plateau of the maximum during which the average
    /// or minimum moves by at least `Δ_stuck`.
    MaxFrozenWhileReferenceMoves,
    /// A sample that violates `Min ≤ Avg ≤ Max`.
    OrderViolated,
    /// A sample with a maximum of at least this temperature.
    MaxAtLeast(f32),
}

/// Parameters the detectors need, from the Guardian's parameter file and
/// the catalog's budgets.
#[derive(Debug, Clone, Copy)]
pub struct OnsetParams {
    pub cycle_ms: u64,
    pub stale_ms: u64,
    pub stuck_reference_change_c: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub t_ms: u64,
    pub description: String,
}

impl Onset {
    /// Finds the onset in samples ordered by time. `None` if the fault never
    /// showed at the Guardian's input.
    pub fn find(
        &self,
        samples: &[(u64, &Temperature)],
        injections: &[(u64, &str)],
        params: &OnsetParams,
    ) -> Option<Found> {
        let pairs = samples.windows(2).map(|w| (w[0], w[1]));
        match self {
            Onset::None => samples.first().map(|(t, s)| Found {
                t_ms: *t,
                description: format!("first sample, sequence {}", s.sequence),
            }),
            Onset::NoInput => samples.is_empty().then(|| Found {
                t_ms: 0,
                description: "no sample reached the Guardian's input".to_owned(),
            }),
            Onset::StreamEnd => samples.last().map(|(t, s)| Found {
                t_ms: t + params.cycle_ms,
                description: format!("stream ended after sequence {}", s.sequence),
            }),
            Onset::Injection(action) => {
                injections
                    .iter()
                    .find(|(_, a)| a == action)
                    .map(|(t, a)| Found {
                        t_ms: *t,
                        description: format!("injection '{a}' by the tool"),
                    })
            }
            Onset::Gap => pairs
                .clone()
                .find(|((t0, _), (t1, _))| t1 - t0 > params.stale_ms)
                .map(|((t0, before), (t1, _))| Found {
                    t_ms: t0 + params.cycle_ms,
                    description: format!(
                        "no sample for {} ms after sequence {}",
                        t1 - t0,
                        before.sequence
                    ),
                }),
            Onset::AliveCounterRepeats => pairs
                .clone()
                .find(|((_, a), (_, b))| a.alive_counter == b.alive_counter)
                .map(|(_, (t, b))| Found {
                    t_ms: t,
                    description: format!("alive counter {} repeated", b.alive_counter),
                }),
            Onset::AliveCounterJumps => pairs
                .clone()
                .find(|((_, a), (_, b))| {
                    let step = (b.alive_counter + 256 - a.alive_counter) % 256;
                    step >= 3
                })
                .map(|((_, a), (t, b))| Found {
                    t_ms: t,
                    description: format!(
                        "alive counter jumped from {} to {}",
                        a.alive_counter, b.alive_counter
                    ),
                }),
            Onset::QualityNotValid => {
                samples
                    .iter()
                    .find(|(_, s)| s.quality != "VALID")
                    .map(|(t, s)| Found {
                        t_ms: *t,
                        description: format!("quality {} at sequence {}", s.quality, s.sequence),
                    })
            }
            Onset::MaxFrozenWhileReferenceMoves => {
                find_frozen_max(samples, params.stuck_reference_change_c)
            }
            Onset::OrderViolated => samples
                .iter()
                .find(|(_, s)| !(s.min_c <= s.avg_c && s.avg_c <= s.max_c))
                .map(|(t, s)| Found {
                    t_ms: *t,
                    description: format!(
                        "Min {} °C, Avg {} °C, Max {} °C out of order",
                        s.min_c, s.avg_c, s.max_c
                    ),
                }),
            Onset::MaxAtLeast(limit) => {
                samples
                    .iter()
                    .find(|(_, s)| s.max_c >= *limit)
                    .map(|(t, s)| Found {
                        t_ms: *t,
                        description: format!("maximum {} °C ≥ {limit} °C", s.max_c),
                    })
            }
        }
    }
}

fn find_frozen_max(samples: &[(u64, &Temperature)], change_c: f32) -> Option<Found> {
    let mut start = 0;
    for index in 1..samples.len() {
        let (_, first) = samples[start];
        let (_, current) = samples[index];
        if current.max_c.to_bits() != first.max_c.to_bits() {
            start = index;
            continue;
        }
        let moved = (current.avg_c - first.avg_c).abs() >= change_c
            || (current.min_c - first.min_c).abs() >= change_c;
        if moved {
            let (t, s) = samples[start];
            return Some(Found {
                t_ms: t,
                description: format!(
                    "maximum frozen at {} °C from sequence {} while the references move",
                    s.max_c, s.sequence
                ),
            });
        }
    }
    None
}

impl fmt::Display for Onset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Onset::None => write!(f, "none"),
            Onset::NoInput => write!(f, "no_input"),
            Onset::StreamEnd => write!(f, "stream_end"),
            Onset::Injection(action) => write!(f, "injection:{action}"),
            Onset::Gap => write!(f, "gap"),
            Onset::AliveCounterRepeats => write!(f, "alive_counter_repeats"),
            Onset::AliveCounterJumps => write!(f, "alive_counter_jumps"),
            Onset::QualityNotValid => write!(f, "quality_not_valid"),
            Onset::MaxFrozenWhileReferenceMoves => write!(f, "max_frozen_while_reference_moves"),
            Onset::OrderViolated => write!(f, "order_violated"),
            Onset::MaxAtLeast(limit) => write!(f, "max_at_least:{limit}"),
        }
    }
}

impl FromStr for Onset {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(match text {
            "none" => Onset::None,
            "no_input" => Onset::NoInput,
            "stream_end" => Onset::StreamEnd,
            "gap" => Onset::Gap,
            "alive_counter_repeats" => Onset::AliveCounterRepeats,
            "alive_counter_jumps" => Onset::AliveCounterJumps,
            "quality_not_valid" => Onset::QualityNotValid,
            "max_frozen_while_reference_moves" => Onset::MaxFrozenWhileReferenceMoves,
            "order_violated" => Onset::OrderViolated,
            other => {
                if let Some(action) = other.strip_prefix("injection:") {
                    Onset::Injection(action.to_owned())
                } else if let Some(limit) = other.strip_prefix("max_at_least:") {
                    Onset::MaxAtLeast(
                        limit
                            .parse()
                            .map_err(|_| format!("invalid temperature in onset {other}"))?,
                    )
                } else {
                    return Err(format!("unknown onset {other}"));
                }
            }
        })
    }
}

impl TryFrom<String> for Onset {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Onset> for String {
    fn from(onset: Onset) -> Self {
        onset.to_string()
    }
}
