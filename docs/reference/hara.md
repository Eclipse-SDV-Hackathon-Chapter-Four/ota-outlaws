<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Preliminary HARA: Battery Thermal Guardian

## Purpose and status

This is a preliminary, software-item-level HARA draft for the Battery Thermal Guardian. It organizes candidate malfunctions and hazardous events for the fault campaigns.

## Item definition

**Item:** Battery Thermal Guardian service.

**Inputs:** Cell-temperature data and its source timestamp, sequence counter, and quality status received through the VSS uProtocol service interface. The Guardian must not read the KUKSA Data Broker directly.

**Outputs:** Diagnostic/fault events.

**Provisional boundary:** The Guardian's evaluation and output requests are in scope. Physical sensors, CAN decoding, KUKSA components, the occupant interface, and physical mitigation actuators are external dependencies unless the system architecture explicitly brings them into the item. Their failure behavior and interfaces still need to be included in the system-level safety analysis.

**Safety-related objective:** Detect developing battery thermal events and provide timely warning and defined mitigation, without treating unavailable or untrusted temperature data as evidence that the battery is safe.

**Considered functionality:** The Battery Thermal Guardian shall monitor battery-cell temperature data, identify developing thermal hazards, and request occupant warnings and defined mitigation actions.

## Terms and traceability

- **Fault:** A cause or defect, such as a dropped message or a biased sensor.
- **Malfunctioning behavior:** The item's function is absent, incorrect, or delayed,
	such as the Guardian failing to identify an unsafe temperature trend.
- **Hazard:** A potential source of harm, such as an escalating battery thermal event without an effective warning.
- **Hazardous event:** A hazard in a specific vehicle operational situation.
- **Harm:** Injury to people, such as burns or smoke inhalation.

## Operational situations to assess

These are candidate situations, not exposure ratings. Assess each relevant
hazard/situation combination using the applicable ISO 26262 edition and vehicle
category.

| ID | Operational situation |
|---|---|
| OS-1 | Occupied vehicle travelling on a public road |
| OS-2 | Occupied vehicle travelling at low speed or manoeuvring |
| OS-3 | Vehicle parked or charging with occupants present |

## Candidate faults and malfunctions

These are the fault campaign inputs. Class labels describe the likely injection layer; they do not establish the HARA classification. A constant value is not a duplicate by itself: use message identity/sequence and timing to detect retransmission. A stuck signal is a signal/source fault unless updates themselves stop, in which case it may also present as a transport timeout.

| ID | Description | Class | Malfunction to evaluate |
|---|---|---|---|
| F-1 | Temperature value remains frozen while messages continue | Signal/source | Guardian accepts stale data and misses a rising temperature |
| F-2 | Message arrives after its allowed age/deadline | Transport | Guardian evaluates old data or warns too late |
| F-3 | Same message is delivered more than once | Transport | Guardian counts a retransmission as a new sample and distorts its trend |
| F-4 | Expected update is dropped before reaching the Guardian (publisher/transport) | Transport | Guardian receives no new sample and may continue using cached data until its freshness timeout |
| F-5 | Messages arrive out-of-order | Transport | Guardian derives an incorrect trend or state transition |
| F-6 | One isolated temperature sample is outside the configured interval; no repeated out-of-range samples or concurrent fault are injected | Signal | Guardian accepts an implausible value and creates a false positive or false negative warning, or fails to preserve a justified warning |
| F-7 | Temperature drifts over time | Signal | Guardian's estimated thermal state or trend is incorrect |
| F-8 | One isolated temperature spike exceeds the configured rate plausibility limit; no repeated spike or concurrent fault is injected | Signal | Guardian misses to issue a justified warning or issues an unjustified warning/mitigation request |
| F-9 | Source disconnects or replay stops | Source/Transport | Guardian fails to identify loss of connection |
| F-10 | Guardian process terminates or its evaluation loop hangs/stops making progress | Application | Guardian stops evaluating temperature and publishing safety events/heartbeat; termination and hang are separate injection variants |
| F-11 | CAN source marks a fresh temperature frame `INVALID` (`0x00`) or `ERROR_NOT_AVAILABLE` (`0xFF`) | Source/Signal | Guardian fails to reject the sample, mark monitoring unavailable, or report the quality fault; invalid temperature data may be treated as trustworthy or its loss may go unnoticed |
| F-12 | Temperature signal saturates at the upper representable value (255 °C) | Signal/Source | Guardian cannot distinguish sensor saturation from a genuinely extreme temperature; failure to apply high out-of-range handling could produce an unsafe assessment, while a conservative warning may be spurious if actual temperature is lower |
| F-13 | Repeated temperature spikes exceed the configured rate plausibility limit within the suspect window | Signal | Guardian fails to escalate persistently implausible input, silently loses trustworthy thermal monitoring, or treats repeated invalid samples as grounds for an unsafe overtemperature mitigation |

> NOTE: Diagnostic-path campaigns such as delayed DFM writes or partial OpenSOVD visibility should be tracked separately as evidence-chain faults. They test whether a scenario is observable and its verdict is supportable; they are not temperature-input malfunctions by themselves.

## Severity Definition

|Name|Description|
|---|---|
| S0 | No harm |
| S1 | Light injuries |
| S2 | Severe injures |
| S3 | Death |

## Hazardous Events

For every hazardous event, assess **S** (severity of potential harm), **E** (exposure to the operational situation), and **C** (controllability of the hazardous event by the driver or other persons at risk). Use the definitions and ASIL determination table from the applicable ISO 26262 edition and vehicle category. Do not infer exposure from fault frequency. Record the rationale and evidence for each rating; derive ASIL only after S/E/C are agreed.

The events below group faults by the unsafe outcome they can produce, rather than treating every injected fault as a separate hazardous event. A fault is listed only for the effect stated in that row; where its direction or system response matters, that condition is noted. Separate rows are retained where the operational situation can change exposure or controllability. Confirm the mappings against the vehicle architecture before assigning ratings.

| ID | Malfunctioning behavior and related faults | Operational situation | Hazardous event and potential harm | S | E | C | ASIL |
|---|---|---|---|---|---|---|---|
| HE-1 | Guardian misses or delays detection because it accepts stale samples (F-1), samples arriving after the warning deadline (F-2), accepts duplicated samples (F-3), continues using data after an upstream omitted update (F-4), derives a misleading trend from reordered samples (F-5), accepts an isolated low out-of-range sample (F-6), accepts a downward temperature drift or isolated spike (F-7, F-8), fails to detect source loss (F-9), stops evaluating and publishing safety events after process termination or hang (F-10), treats invalid source quality as trustworthy or fails to report monitoring loss (F-11), or fails to report monitoring loss after repeated spikes are discarded (F-13)). | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | Battery fire or smoke develops before occupants receive a usable warning and can stop in a safe place or evacuate; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-2 | Same missed/delayed-detection effects and fault conditions as HE-1, including invalid source quality (F-11), Guardian termination or hang (F-10), and failure to report monitoring loss after repeated spikes (F-13) | OS-2: Occupied vehicle manoeuvring at low speed near other vehicles or pedestrians | Battery fire or smoke develops before occupants can safely stop or move away; occupants or nearby people may be exposed to smoke or heat while the vehicle is manoeuvring. | S3 | - | - | ASIL-D |
| HE-3 | Same missed/delayed-detection effects and fault conditions as HE-1, including invalid source quality (F-11), Guardian termination or hang (F-10), and failure to report monitoring loss after repeated spikes (F-13). | OS-3: Vehicle parked or charging with occupants in or immediately beside it | Battery fire or smoke develops before occupants receive a usable warning and can leave the vehicle or nearby area; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-4 | Guardian issues an unintended mitigation request because it counts a duplicate as a new sample (F-3), accepts an isolated high out-of-range value (F-6), or treats an isolated or repeated high spike as valid/critical (F-8, F-13). | OS-1: Occupied vehicle moving in road traffic | The unintended request changes vehicle response or interrupts propulsion, preventing the driver from maintaining a safe trajectory and creating a collision risk for occupants or other road users. | S3 | - | - | ASIL-D |
| HE-5 | Same unintended-mitigation effects and conditions as HE-4 | OS-2: Occupied vehicle manoeuvring near other vehicles or pedestrians | The unintended request changes vehicle response during a manoeuvre, creating a collision risk for occupants or nearby road users. | S3 | - | - | ASIL-D |
| HE-6 | Temperature drifts upwards (F-7), one isolated temperature sample is outside the upper limit (F-6), an isolated or repeated spike appears (F-8, F-13), or an upper-bound-saturated sample triggers a warning although actual temperature is below the dangerous range (F-12). | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | A warning that is not supported by actual thermal danger distracts the driver and may increase collision risk; an upper-bound reading must still be treated cautiously because it may represent real danger. | S3 | - | - | ASIL-D |

F-11 maps to HE-1 through HE-3 because it can make thermal monitoring
unavailable during the same hazardous vehicle situations; it does not create a
separate hazardous event. The source quality flag is expected to make the
sample unusable; the hazardous malfunction is that the Guardian accepts it as
trustworthy or fails to report the resulting loss of monitoring. S3 is plausible
only when a real thermal event coincides with the loss of monitoring. E and C
remain unassessed, so the displayed ASIL-D values are not fully derived or
confirmed for F-11 until exposure and controllability are supported for each
operating situation.

F-12 maps to HE-6 only for the conditional false-warning path; the correct
high-out-of-range response remains a warning because real danger cannot be
excluded.

F-13 maps to HE-1 through HE-3 when repeated spikes are discarded without
escalating monitoring loss, and to HE-4 through HE-6 if invalid spikes instead
cause an unsafe mitigation or unsupported warning. Its expected response is
the repeated-invalid escalation in FSR-3.5, while FSR-3.6 prohibits invalid-only
overtemperature mitigation. The isolated F-6 and F-8 campaigns do not combine
their fault with another fault or repeat the injected anomaly.

## Risk classification and safety goals

Candidate safety goals are item/vehicle-level objectives. They are not one-to-one
requirements to detect each injected fault; detailed detection and handling
requirements belong in the functional safety concept and technical design.

| ID | Related hazardous events | Candidate safety goal |
|---|---|---|
| SG-1 | HE-1, HE-2, HE-3 | The vehicle shall provide an effective occupant warning and defined mitigation for a developing battery thermal event before occupants are exposed to an unacceptable risk. |
| SG-2 | HE-1, HE-2, HE-3 | Loss or invalidity of thermal monitoring, including Guardian unavailability, shall not be interpreted as a safe battery condition. The vehicle shall detect the loss independently of the failed Guardian and perform a defined warning/degraded response. |
| SG-3 | HE-4, HE-5 | Invalid, repeated, or duplicated temperature data shall not cause an unintended mitigation action that creates an unsafe change in vehicle response. |
| SG-4 | HE-6, HE-1 | The vehicle shall prevent an invalid temperature sample from causing an unsafe driver or vehicle response, while preserving timely warnings when the sample may indicate a real thermal danger. |


## Derived Functional Requirements

The following candidate requirements are derived from SG-1 to SG-4. They refine
the HARA goals for traceability; the referenced Safety Concept remains the source
for existing Guardian FSR wording, parameter values, priority, and status.

| ID | HARA trace | Derived functional requirement | Safety Concept mapping |
|---|---|---|---|
| DFR-1 | SG-1; HE-1 to HE-3 | The Guardian shall detect each approved thermal-risk criterion and issue the corresponding occupant warning and mitigation response within its approved (`T_react = 100ms`). | FSR-1.1 to FSR-1.4 cover threshold, critical, trend, and hot-spot detection; FSR-1.6 and FSR-1.7 cover mitigation state/failure behavior. |
| DFR-2 | SG-2; HE-1 to HE-3; F-11, F-12 | The Guardian shall treat a fresh VSS/uProtocol sample whose mapped CAN quality is `INVALID` or `ERROR_NOT_AVAILABLE`, or whose value is outside the configured range, as unusable, enter `DEGRADED`, and report the corresponding fault within `T_react`; it shall not treat the sample as evidence that the battery is safe. A high out-of-range value shall still raise at least `WARNING`. | FSR-3.2 and FSR-3.4; general loss handling in FSR-2.1 to FSR-2.6. |
| DFR-3 | SG-2, SG-4; HE-1 to HE-3; F-11 | Invalid or degraded input, including a CAN quality flag other than `VALID`, shall not lower or clear an active thermal warning. Thermal state may be lowered only after the defined recovery conditions are met using valid samples. | FSR-1.5, FSR-2.5, FSR-2.6, and FSR-3.6. |
| DFR-4 | SG-3; HE-4, HE-5, HE-6; F-6, F-8, F-12, F-13 | An isolated or repeated invalid, stale, duplicated, out-of-order, or saturated-high sample shall not by itself cause `CRITICAL` or an overtemperature mitigation. A saturated/high out-of-range value shall still cause at least `WARNING` because real danger cannot be excluded. Repeated spikes shall escalate monitoring to `DEGRADED` and report monitoring unavailable, without lowering the thermal state. | FSR-1.2, FSR-2.5, FSR-3.2, FSR-3.3, FSR-3.5, and FSR-3.6. |
| DFR-5 | SG-1, SG-2; HE-1 to HE-3; F-10 | An independent in-vehicle supervisor shall detect Guardian termination or loss of evaluation progress and request the defined monitoring-unavailable occupant warning through a path that does not depend on the Guardian or Evidence Collector. | FSR-2.7 only requires heartbeat observation by the Evidence Collector and restart of a terminated Guardian. |
| DFR-6 | Diagnostic goal; all faulted events | Each detected fault shall be traceable from Guardian/equipment event through DFM and OpenSOVD to the campaign verdict; diagnostic failures shall not delay safety reactions. | FSR-D.1 to FSR-D.4; EC-1 to EC-3. |
| DFR-7 | F-3 | After two messages with the same counter the monitoring state `SUSPECT` is reported, after 10 messages it switchs to `DEGRADED`| |
| DRF-8 | F-3 | After messages with same counter values are received and ten messages with monotonic increasing counter are received, signal state recovers to `OK`| |
| DFR-9 | SG-1, SG-2; HE-1 to HE-3 | The temperature source/publisher shall identify a sample clipped below the CAN representation range (rather than a genuine `0 °C` measurement) and propagate that indication to the Guardian. If this cannot be provided, the vehicle/system safety analysis shall justify that treating the lower-bound value as valid cannot delay warning for applicable cold-operation thermal profiles. | No matching source/publisher requirement or metadata exists in the current Safety Concept/protocol. |

## HARA-derived test scenarios

Run each fault variant independently from a fresh Guardian instance
unless a scenario explicitly tests recovery. Record the active configuration and
observe the same input stream the Guardian receives. Timing parameters refer to
the approved Safety Concept configuration.

| HARA fault | Expected mitigation from test specifications | Covering test cases |
|---|---|---|
| F-1: Temperature value remains frozen while messages continue | No detection is demonstrated. TS-10 lists `DriverWarningMonitoringUnavailable` but explicitly treats the all-values-frozen case as a limitation probe. | TS-10 (limitation probe only) |
| F-2: Message arrives after its allowed age/deadline | `DriverWarningMonitoringUnavailable` when the stale stream causes `DEGRADED`. | TS-05 covers a prolonged update gap, but not explicit rejection of a late-arriving message; proposed TS-24. |
| F-3: Same message is delivered more than once | No mitigation for one duplicate; `DriverWarningMonitoringUnavailable` if repeated counters escalate monitoring to `DEGRADED`. | TS-07 |
| F-4: Expected update is dropped before reaching the Guardian | `DriverWarningMonitoringUnavailable`; TS-06 also lists `DiscardSample`. | TS-05, TS-06, TS-15 |
| F-5: Messages arrive out-of-order | `DriverWarningMonitoringUnavailable` if monitoring degrades; the stale sample must not update thermal assessment. | TS-08 |
| F-6: One isolated temperature sample is outside the configured interval | No overtemperature mitigation from invalid input alone; preserve an active thermal warning and report `DriverWarningMonitoringUnavailable` if monitoring becomes `DEGRADED`. | TS-12, TS-17, TS-18 |
| F-7: Temperature drifts over time | No dedicated drift mitigation is specified by an existing test. Proposed TS-25 expects a `WARNING` for a sustained rising trend below `θ_warn`; no overtemperature mitigation before the critical criterion. | No dedicated fault test; TS-02 tests threshold heating, not drift detection. Proposed TS-25. |
| F-8: One isolated temperature spike exceeds the rate plausibility limit | No overtemperature mitigation from the isolated invalid sample; `DiscardSample` is catalog-proposed, and `DriverWarningMonitoringUnavailable` applies only if the configured response degrades monitoring. | TS-12, TS-19, TS-20 |
| F-9: Source disconnects or replay stops | `DriverWarningMonitoringUnavailable`. | TS-03, TS-04, TS-15 |
| F-10: Guardian terminates or evaluation hangs | `RestartGuardian` only where the runtime restart policy applies; no independent occupant warning is demonstrated. | TS-22 (termination), TS-23 (hang) |
| F-11: CAN source marks a fresh temperature frame invalid or unavailable | `DriverWarningMonitoringUnavailable`; no overtemperature mitigation from invalid quality alone. | TS-11, TS-16 |
| F-12: Temperature signal saturates at 255 °C | At least `WARNING`; `DriverWarningMonitoringUnavailable` if monitoring becomes `DEGRADED`; no overtemperature mitigation from invalid data alone. | No explicit 255 °C test. TS-12 only tests a generic high out-of-range sample; proposed TS-26. |
| F-13: Repeated temperature spikes exceed the rate limit within the suspect window | `DriverWarningMonitoringUnavailable`; no overtemperature mitigation from invalid spikes alone. | TS-21 |

### Test Template

**HARA trace:** 

**Preconditions:**

**Stimulus:**

**Expected Result:**

**Expected Mitigations:**

**Evidence and Verdict Focus:**

### TS-01: Baseline nominal monitoring

**HARA trace:** DRF-1 - DRF-4

**Preconditions:** Start the system, subscribe to the mitigation topic

**Stimulus:** Publish valid, in-range, steadily updated temperatures below warning thresholds with valid quality (0x80) and monotonic increasing counter.

**Expected Result:** Initial thermal state is `CLEAR`. Monitoring becomes `OK` and thermal state becomes `MONITORING`. No warning, mitigation, or fault is emitted.

**Expected Mitigations:** None

**Evidence and Verdict Focus**: Capture input/output stream, fail on unexpected fault, warning or emitted mitigation.

### TS-02: Thermal warning and critical thresholds

**HARA trace:** HE-1 to HE-3; SG-1; DFR-1.

**Preconditions:** Start the Guardian with a valid temperature stream and
establish `MONITORING` below `θ_warn`. Subscribe to the mitigation topic.

**Stimulus:** Replay a valid rising-temperature profile through `θ_warn` and
then `θ_crit`.

**Expected Result:** The Guardian enters state `WARNING` at `θ_warn`, then enters
state `CRITICAL` and emits `DriverWarningOvertemp` mitigation at `θ_crit`. Each response meets
the configured `T_react` budget.

**Expected Mitigations:** `DriverWarningOvertemp`

**Evidence and Verdict Focus:** Record the threshold-crossing samples, state
transitions, warning event, and input-to-output latency. This verifies threshold response and mitigations.

### TS-03: No data after startup

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the Battery Thermal Guardian without starting the CAN
temperature source.

**Stimulus:** Publish no temperature data and allow the startup timeout to
expire.

**Expected Result:** The Guardian enters `DEGRADED` and reports the corresponding
startup/connection-loss fault within the configured timeout `T_stale` and reaction budget `T_react`. Reports mitigation `DriverWarningMonitoringUnavailable`

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Record Guardian startup time, confirm no samples
reached its input, and capture the status transition, fault event, DFM record,
and latency.

### TS-04: Temperature source shutdown

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source and Guardian. Publish valid, in-range,
steadily updated data for at least `3 sec`.

**Stimulus:** Shut down the CAN source while leaving the Guardian running.

**Expected Result:** After no fresh sample arrives for `T_stale`, the Guardian
enters `DEGRADED` and reports the corresponding freshness fault within the
configured reaction budget.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` 

**Evidence and Verdict Focus:** Record the last valid sample, source shutdown,
Guardian-input timeout, Guardian status/fault event, DFM record, and measured
latency.

### TS-05: Delayed or withheld update

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source, Data Broker, VSS Publisher, Guardian, OpenSOVD
and Evidence Collector tap points. Verify data is visible at the source and
publisher output and reaches the Guardian.

**Stimulus:** Delay or withhold an expected update so no fresh data reaches the
Guardian for longer than `T_stale`.

**Expected Result:** The Guardian enters `DEGRADED` and reports a freshness fault
within the configured reaction budget.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Record source timestamps and arrival times, the
Guardian-input gap, `T_stale`, status/fault event, DFM record, and latency.
Confirm the delayed stimulus reached the Guardian input.

### TS-06: Publisher-to-Guardian transport dropout

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source, Data Broker, VSS Publisher, Guardian, OpenSOVD
and Evidence Collector tap points. Verify data is visible at the source and
publisher output and reaches the Guardian.

**Stimulus:** Inject a publisher/transport dropout between publisher output and
the Guardian input while keeping the source and publisher output active.

**Expected Result:** The Guardian enters `DEGRADED` and reports a freshness fault
after `T_stale`, within the configured reaction budget.

**Expected Mitigations:** `DiscardSample`, `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Compare source, publisher-output, and
Guardian-input observations. Capture the Guardian event and DFM record; use the
tap-point differences to attribute the dropout to the publisher-to-Guardian
path. Mark attribution inconclusive if the observations cannot distinguish it
from source loss.

### TS-07: Duplicate message

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2 and DFR-6; FSR-2.2,
FSR-3.7, and EC-2.

**Preconditions:** Start the CAN source, VSS Publisher, Guardian, and Evidence
Collector. Publish valid, in-range samples until monitoring is `OK` and the
collector observes the Guardian-input sequence.

**Stimulus:** Deliver the same published message twice with the same publisher
sequence counter, signal quality and payload.

**Expected Result:** The Guardian does not accept the duplicate as a fresh
sample or advance its thermal assessment from it. The Evidence Collector flags
the repeated publisher sequence. A single duplicate followed by fresh samples
does not cause a freshness timeout. After two messages with the same counter
(one duplicate), the Guardian reports `SUSPECT` monitoring state. After ten
messages with the same counter, the Guardian reports `DEGRADED`.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Capture publisher sequence, source timestamp,
alive counter, Guardian-input arrival order, Guardian state/fault events, DFM
record, and collector attribution. A duplicate not visible at the Guardian-
input tap is an inconclusive injection, not a pass.

### TS-08: Out-of-order message

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2 and DFR-6; FSR-2.4.

**Preconditions:** Start the CAN source, VSS Publisher, Guardian, and Evidence
Collector. Publish valid samples with increasing publisher sequence and source
timestamps until monitoring is `OK`.

**Stimulus:** Deliver a previously published sample after a newer sample, so
the publisher counter decreases at the Guardian input.

**Expected Result:** After one decreasing counter, the Evidence Collector flags the sequence regression as `SUSPECT`. The Guardian ignores the sample as non-fresh because its source timestamp is older,
and does not use it to update thermal assessment. After ten messages with the same counter, the Guardian reports `DEGRADED`.

**Expected Mitigations:**  `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Capture publisher sequence, source timestamp,
alive counter, arrival order, Guardian-input acceptance/state, collector
sequence-regression report, and any freshness fault/DFM record. If only the
publisher sequence regresses while source timestamp and alive counter remain
fresh, record that the current Guardian freshness contract does not reject it;
do not claim Guardian-side out-of-order rejection for that variant.

---

### TS-09: Stuck maximum temperature

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2 and DFR-6; FSR-2.4.

**Preconditions:** Start the source and Guardian with valid, steadily updated
maximum, average, and minimum temperatures. Record the configured `T_stuck` and
`Δ_stuck`.

**Stimulus:** Hold the maximum-temperature value constant while changing the
average or minimum by at least `Δ_stuck`. Repeat using both fast and slow heating
profiles. Include a slow nominal heating profile where the maximum is not stuck
as a negative control.

**Expected Result:** Once the maximum has remained unchanged for `T_stuck` and
the other signal has moved by at least `Δ_stuck`, the Guardian enters `DEGRADED`
and reports a signal-stuck fault within the specified hidden-error bound. The
negative control does not produce a stuck fault.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` when the stuck
fault causes monitoring to become `DEGRADED`.

**Evidence and Verdict Focus:** Record all three temperature signals, source
timestamps, detection time, the movement of average/minimum at detection,
`T_stuck`, `Δ_stuck`, Guardian status/fault, and diagnostic record. Confirm the
hidden temperature error remains within `Δ_stuck` plus one CAN step.

### TS-10: All temperature values frozen probe

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2; F-1 limitation probe.

**Preconditions:** Start the source and Guardian with fresh timestamps and an
advancing source alive counter. Establish normal monitoring before the probe.

**Stimulus:** Keep all temperature values constant while continuing to advance
timestamps and the source alive counter.

**Expected Result:** Characterize whether the current design can distinguish a
genuinely constant battery temperature from a fully frozen set of temperature
signals. The currently specified stuck-maximum check requires another
temperature channel to change, so it is not expected to identify this case by
itself.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Record all temperature values, timestamps,
alive-counter values, monitoring status, and emitted faults. Treat this as a
coverage/limitation result, not a passing detection test; do not claim F-1 is
detected unless an implemented mechanism actually detects it.

### TS-11: Invalid source quality

**HARA trace:** F-11; HE-1 to HE-3; SG-2 and SG-4; DFR-2 and DFR-3;
FSR-3.4 and FSR-3.6.

**Preconditions:** Start a fresh Guardian with valid samples and establish
`WARNING`; repeat from `CRITICAL` to verify thermal-state retention.

**Stimulus:** In a separate run, replay one CAN frame whose raw `Quality` byte
is `0x00` (`INVALID`) or `0xFF` (`ERROR_NOT_AVAILABLE`), with fresh timestamp
and advancing alive counter. Verify the VSS Publisher maps that source quality
to the corresponding uProtocol quality enum. Then restore fresh valid frames.

**Expected Result:** The Guardian rejects the sample, sets monitoring status to
`DEGRADED`, and reports a quality fault within `T_react`. The thermal state is
not lowered by the invalid sample. Monitoring recovers only after the configured
consecutive-valid-sample rule; thermal-state reduction still follows hysteresis.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` while monitoring
is `DEGRADED`; no new overtemperature mitigation is caused by invalid quality
alone.

**Evidence and Verdict Focus:** Capture the raw CAN quality byte, mapped VSS /
uProtocol quality enum, source timestamp, alive counter, thermal state
before/after, monitoring-status transition, fault code, DFM record, recovery
sample count, and response latency. The catalog maps this case to F-11.

### TS-12: High anomalous samples and mitigation gating

**HARA trace:** HE-1, HE-4, and HE-5; SG-1 and SG-3; DFR-1 and DFR-4;
FSR-3.2, FSR-3.3, and FSR-3.6.

**Preconditions:** Start the Guardian with valid input and a thermal state below
`CRITICAL`. Confirm the configured upper range and `r_max`.

**Stimulus:** Run separate variants: inject a high out-of-range temperature
sample, then inject a one-sample temperature rise exceeding `r_max`. Provide a
valid critical-temperature sample as a positive control.

**Expected Result:** Each anomalous high sample raises at least `WARNING`, since
it may indicate a real thermal event. An invalid sample alone does not raise
`CRITICAL` or cause mitigation. The valid critical positive control reaches
`CRITICAL` and produces the defined mitigation request.

**Expected Mitigations:** No mitigation request for either invalid high sample
alone. `DriverWarningOvertemp` for the valid critical positive control.

**Evidence and Verdict Focus:** Capture sample validity, quality, temperature
values, configured limits, rise rate, thermal-state transitions, warning and
mitigation events, fault record, and reaction latency. Do not classify the
expected `WARNING` as a false positive solely because the injected sample was
anomalous.

### TS-13: Overtemperature warning

**HARA trace:** HE-1 to HE-3; SG-1; catalog fault: Overtemperature Warning.

**Preconditions:** Start the Guardian with valid, fresh, in-range input below
`θ_warn` and subscribe to thermal-state and mitigation events.

**Stimulus:** Increase only the valid maximum cell temperature to `θ_warn`,
keeping it below `θ_crit`.

**Expected Result:** The thermal state becomes `WARNING`; it does not become
`CRITICAL`.

**Expected Mitigations:** None, matching the catalog's stated current behavior.

**Evidence and Verdict Focus:** Record the valid threshold-crossing sample,
state transition, any DTC, and emitted mitigation events. Fail if this
warning-only condition triggers critical mitigation.

### TS-14: Overtemperature critical

**HARA trace:** HE-1 to HE-3; SG-1; catalog fault: Overtemperature Critical.

**Preconditions:** Start with valid input and thermal state `WARNING`; subscribe
to Guardian state, DTC, and mitigation outputs.

**Stimulus:** Raise only the valid maximum cell temperature to `θ_crit`.

**Expected Result:** The Guardian enters `CRITICAL` within the approved reaction
budget and reports the overtemperature-critical DTC.

**Expected Mitigations:** `DriverWarningOvertemp`.

**Evidence and Verdict Focus:** Capture the valid sample, threshold, state
transition, DTC/DFM record, mitigation event, and latency.

### TS-15: Freshness lost

**HARA trace:** HE-1 to HE-3; SG-2; F-4/F-9; catalog fault: FreshnessLost.

**Preconditions:** Establish normal `OK` monitoring with valid samples arriving
at the Guardian input.

**Stimulus:** Interrupt the sample stream at the Guardian input for longer than
`T_stale`.

**Expected Result:** The Guardian enters `DEGRADED` and reports a freshness
fault within `T_stale + T_react`; it does not lower an existing thermal state.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`.

**Evidence and Verdict Focus:** Record the last valid sample, tap-point outage
onset, timeout, Guardian status/fault, DFM/OpenSOVD evidence, and latency. Inject
only the stream interruption and use taps to attribute its origin.

### TS-16: Quality invalid

**HARA trace:** HE-1 to HE-3; SG-2; F-11; catalog fault: QualityInvalid.

**Preconditions:** Establish valid monitoring with fresh samples.

**Stimulus:** Publish one fresh sample whose quality is `INVALID` or
`ERROR_NOT_AVAILABLE`, with no other sample field or transport fault.

**Expected Result:** The Guardian rejects the sample, enters `DEGRADED`, and
reports a quality-invalid fault within `T_react`. Thermal state is not lowered.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`; no new
overtemperature mitigation from invalid quality alone.

**Evidence and Verdict Focus:** Record quality, source timestamp, alive counter,
state/status transition, DTC, DFM/OpenSOVD record, and latency. F-11 provides
the candidate malfunction trace for this scenario.

### TS-17: Out-of-range low value

**HARA trace:** HE-1 to HE-3; SG-2/SG-4; F-6; catalog fault: OutOfRange.

**Preconditions:** Establish `WARNING` with otherwise valid input and confirm
the approved `[θ_min, θ_max]` range.

**Stimulus:** In this isolated-fault scenario, inject exactly one fresh,
quality-valid `CellTempMin` below `θ_min`, with the other sample fields in
range, then resume valid in-range samples. Do not combine it with another fault.

**Expected Result:** The sample is rejected and an out-of-range fault is
reported. The active thermal state is not lowered. Monitoring follows the
approved invalid-input response.

**Expected Mitigations:** Preserve the active warning; report
`DriverWarningMonitoringUnavailable` if monitoring enters `DEGRADED`.

**Evidence and Verdict Focus:** Record range limits, injected value, state
before/after, monitoring status, fault, diagnostic record, and latency.

### TS-18: Out-of-range high value

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-6; catalog fault: OutOfRange.

**Preconditions:** Establish `MONITORING` below `CRITICAL` with valid input and
confirm `θ_max`.

**Stimulus:** In this isolated-fault scenario, inject exactly one fresh sample
with `CellTempMax` above `θ_max`; keep quality, timestamp, and counters valid,
then resume valid in-range samples. Do not combine it with another fault.

**Expected Result:** The sample is rejected and reported out of range. Thermal
state is at least `WARNING`, since real danger cannot be excluded, but invalid
input alone does not cause `CRITICAL` or mitigation.

**Expected Mitigations:** No overtemperature mitigation from this sample alone;
monitoring-unavailable warning if monitoring becomes `DEGRADED`.

**Evidence and Verdict Focus:** Record value, limit, state/status, DTC, warning,
mitigation events, and latency. Fail if the warning is suppressed or invalid
input alone triggers mitigation.

### TS-19: Rate-implausible sample

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-8; catalog fault:
RateImplausible.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`r_max`, `Δ_res`, and timestamp configuration.

**Stimulus:** In this isolated-fault scenario, inject exactly one fresh sample
whose maximum-temperature rise exceeds `r_max × Δt + Δ_res`; do not inject any
other invalid field or repeat the spike within `T_suspect`.

**Expected Result:** The sample is rejected and a rate-implausible fault is
reported. The state rises to at least `WARNING` because real runaway cannot be
excluded; the invalid sample alone does not cause `CRITICAL` or mitigation.

**Expected Mitigations:** No mitigation from this sample alone;
`DriverWarningMonitoringUnavailable` if the approved response degrades
monitoring.

**Evidence and Verdict Focus:** Record old/new values, source timestamps, Δt,
computed rate, state/status, warning/DTC, mitigation, and latency. Resolve the
FSR-3.3 versus FSR-3.5 isolated-invalid status rule before final pass/fail.

### TS-20: Isolated spike

**HARA trace:** HE-1, HE-4, HE-5, and HE-6; SG-1/SG-3/SG-4; isolated F-8;
catalog fault: Isolated Spike.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`r_max`, `N_suspect`, and `T_suspect`.

**Stimulus:** Inject exactly one fresh sample with a rise above `r_max`, then
resume valid nominal samples. Do not inject any other fault or another spike
within `T_suspect`.

**Expected Result:** The sample is discarded and monitoring becomes `SUSPECT`
under FSR-3.5. At least `WARNING` is retained because the rise may be real; no
invalid-only `CRITICAL` or overtemperature mitigation is allowed. This is a
planned behavior until FSR-3.5 is implemented and the FSR-3.3 interaction is
resolved.

**Expected Mitigations:** `DiscardSample` is catalog-proposed; record the
actual event. No monitoring-unavailable warning for this one isolated spike.

**Evidence and Verdict Focus:** Record the single spike, surrounding valid
samples, status/state, warning/DTC, and mitigation. A pass requires reconciling
the immediate `DEGRADED` response in FSR-3.3 with the `SUSPECT` debounce in
FSR-3.5; until then report the requirement result as blocked/inconclusive.

### TS-21: Repeated spikes

**HARA trace:** HE-1 to HE-6; SG-1/SG-2/SG-3/SG-4; F-13; catalog:
Repeated Spikes.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`N_suspect` and `T_suspect`.

**Stimulus:** Inject only repeated rate-implausible spikes within `T_suspect`,
reaching `N_suspect`; do not combine them with another fault. Follow with valid
samples to exercise recovery.

**Expected Result:** Before `N_suspect` spikes within `T_suspect`, monitoring is
`SUSPECT` and each invalid sample is discarded. At the threshold, monitoring
becomes `DEGRADED` and a signal/rate fault is reported. Thermal state is not
lowered; invalid spikes alone do not cause `CRITICAL` or overtemperature
mitigation. Monitoring recovers only after the configured valid-sample
recovery conditions.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`; no
overtemperature mitigation from invalid spikes alone.

**Evidence and Verdict Focus:** Record every spike, timestamps, debounce
window/count, status/state, thermal state, DTC, DFM record, and output events.
Because FSR-3.5 is planned and FSR-3.3 currently requires immediate
`DEGRADED`, report the implementation verdict as blocked until that requirement
conflict is resolved.

### TS-22: Guardian process termination

**HARA trace:** HE-1 to HE-3; SG-1/SG-2; F-10; catalog: Heartbeat Loss.

**Preconditions:** Start the Guardian, runtime, and Evidence Collector with
valid input active; confirm heartbeat receipt.

**Stimulus:** Terminate the Guardian process once; inject no other fault.

**Expected Result:** The Evidence Collector detects heartbeat loss and the
runtime applies its configured restart policy. No independent in-vehicle
occupant warning is implemented, so occupant protection is not demonstrated.

**Expected Mitigations:** `RestartGuardian` only if the configured runtime
actually restarts it; independent warning remains a safety gap.

**Evidence and Verdict Focus:** Record last heartbeat, timeout, restart,
process recovery, and collector event. Do not treat collector evidence as an
occupant safety response.

### TS-23: Guardian evaluation hang

**HARA trace:** HE-1 to HE-3; SG-1/SG-2; F-10; catalog: Heartbeat Loss.

**Preconditions:** Start Guardian and confirm evaluation progress and heartbeat
while valid input is active.

**Stimulus:** Pause only the Guardian evaluation loop while leaving its process
alive.

**Expected Result:** Pass only if the heartbeat is tied to evaluation progress
and a monitor detects its loss. Current requirements do not define hang
detection or recovery.

**Expected Mitigations:** Runtime recovery only if progress-aware supervision
is implemented; occupant warning is not currently provided.

**Evidence and Verdict Focus:** Distinguish process liveness from evaluation
progress and heartbeat. Mark blocked/failed against the catalog claim until
hang detection/recovery is specified and implemented.

### TS-24: Late-arriving stale message

**HARA trace:** HE-1 to HE-3; SG-2; F-2; FSR-2.8.

**Preconditions:** Enable source/Guardian clock synchronization and configure
`T_age` and `T_stale`. Establish valid monitoring with a fresh sample stream.

**Stimulus:** In an isolated run, hold back one sample until its source
timestamp is older than `T_age`, then deliver it while withholding other fresh
samples until the freshness timeout is reached. Resume with valid fresh samples.

**Expected Result:** The late sample is not accepted as fresh or used to update
the thermal assessment. The Guardian enters `DEGRADED` after the configured
freshness timeout and recovers only under the valid-sample recovery conditions.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` while monitoring
is `DEGRADED`; no overtemperature mitigation from the stale sample alone.

**Evidence and Verdict Focus:** Record source timestamp, delivery time, measured
sample age, `T_age`, `T_stale`, Guardian acceptance, status/state transitions,
fault/DFM record, mitigation, and recovery. This proposal depends on synchronized
clocks and FSR-2.8 being implemented.

### TS-25: Gradual temperature drift

**HARA trace:** HE-1 to HE-3 and HE-6; SG-1/SG-4; F-7; FSR-1.3.

**Preconditions:** Establish valid monitoring below `θ_warn`; configure and
record `r_trend`, `T_trend`, and `T_react`.

**Stimulus:** Replay a valid, gradual, monotonic temperature rise that stays
below `θ_warn` but satisfies the configured trend criterion for `T_trend`.

**Expected Result:** The thermal state becomes `WARNING` within
`T_trend + T_react`; it does not become `CRITICAL` solely because of the trend.

**Expected Mitigations:** No overtemperature mitigation before a valid critical
criterion is reached.

**Evidence and Verdict Focus:** Record all valid samples, temperature slope,
trend window, threshold configuration, state transition, and latency. FSR-1.3
is planned, so this remains a proposed requirement-gap test until implemented.

### TS-26: Upper-scale saturation

**HARA trace:** HE-1 and HE-6; SG-2/SG-4; F-12; DFR-2/DFR-4; FSR-3.2/FSR-3.6.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record the
configured `[θ_min, θ_max]` range and verify the normal CAN-to-VSS/uProtocol path.

**Stimulus:** In an isolated run, send one fresh, quality-valid sample with
`CellTempMax` at the upper representable value (255 °C), then resume valid
in-range samples.

**Expected Result:** The sample is not accepted as a valid in-range measurement;
monitoring becomes `DEGRADED` and thermal state is at least `WARNING`. Invalid
input alone does not cause `CRITICAL` or lower an existing thermal state.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` while monitoring
is `DEGRADED`; no overtemperature mitigation from the invalid sample alone.

**Evidence and Verdict Focus:** Capture the raw CAN value, mapped Guardian
input, quality, timestamps/counter, state/status, warning, fault/DFM record,
mitigation, and latency. Keep this distinct from a generic high out-of-range
sample; the test does not prove that saturation can be distinguished from a
genuinely extreme temperature.

## AI Assistance

This document was revised with the assistance of **GitHub Copilot (GPT-6 Luna)**.
The fault mitigation and test coverage overview was added with the assistance of
**GitHub Copilot** using the model **GPT-6 Luna**.

Removing TS-11/TS-12, aligning TS-07 with DFR-7, and removing the coverage
assessment column were done with the assistance of **Claude Code** using the
model **Claude Opus 5.5** (`claude-opus-5-5`).
