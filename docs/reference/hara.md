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

Fault-campaign identifiers below are not themselves HARA hazards. The intended
trace is: injected fault -> malfunctioning behavior -> hazardous event -> safety
goal -> detection/mitigation evidence -> verdict.

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
| F-6 | Temperature is outside configured interval | Signal | Guardian accepts an implausible value and creates false positive or false negative warning |
| F-7 | Temperature drifts over time | Signal | Guardian's estimated thermal state or trend is incorrect |
| F-8 | Temperature has an implausible spike | Signal | Guardian misses to issue an unjustified warning/mitigation request |
| F-9 | Source disconnects or replay stops | Source/Transport | Guardian fails to identify loss of connection |
| F-10 | Guardian process terminates or its evaluation loop hangs/stops making progress | Application | Guardian stops evaluating temperature and publishing safety events/heartbeat; termination and hang are separate injection variants |

> NOTE: Diagnostic-path campaigns such as delayed DFM writes or partial OpenSOVD visibility should be tracked separately as evidence-chain faults. They test whether a scenario is observable and its verdict is supportable; they are not temperature-input malfunctions by themselves.

## Severity Definition

|Name|Description|
|---|---|
| S0 | No harm |
| S1 | Light injuries |
| S2 | Severe injures |
| S3 | Death |

## Hazardous Events

For every hazardous event, assess **S** (severity of potential harm), **E** (exposure to the operational situation), and **C** (controllability of the hazardous event by the driver or other persons at risk). Use the definitions and ASIL determination table from the applicable ISO 26262 edition and vehicle category. Do not infer exposure from fault frequency. Record the rationale and evidence for each rating; derive ASIL only after S/E/C are agreed. **Most ratings
remain TBD in this draft.**

The events below group faults by the unsafe outcome they can produce, rather than treating every injected fault as a separate hazardous event. A fault is listed only for the effect stated in that row; where its direction or system response matters, that condition is noted. Separate rows are retained where the operational situation can change exposure or controllability. Confirm the mappings against the vehicle architecture before assigning ratings.

| ID | Malfunctioning behavior and related faults | Operational situation | Hazardous event and potential harm | S | E | C | ASIL |
|---|---|---|---|---|---|---|---|
| HE-1 | Guardian misses or delays detection because it accepts stale samples (F-1), samples arriving after the warning deadline (F-2), accepts duplicated samples (F-3), continues using data after an upstream omitted update (F-4), derives a misleading trend from reordered samples (F-5), accepts a low out-of-range value (F-6), accepts a downward temperature drift or spike (F-7, F-8), fails to detect source loss (F-9), or stops evaluating and publishing safety events after process termination or hang (F-10). | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | Battery fire or smoke develops before occupants receive a usable warning and can stop in a safe place or evacuate; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-2 | Same missed/delayed-detection effects and fault conditions as HE-1, including Guardian termination or hang (F-10) | OS-2: Occupied vehicle manoeuvring at low speed near other vehicles or pedestrians | Battery fire or smoke develops before occupants can safely stop or move away; occupants or nearby people may be exposed to smoke or heat while the vehicle is manoeuvring. | S3 | - | - | ASIL-D |
| HE-3 | Same missed/delayed-detection effects and fault conditions as HE-1, including Guardian termination or hang (F-10) | OS-3: Vehicle parked or charging with occupants in or immediately beside it | Battery fire or smoke develops before occupants receive a usable warning and can leave the vehicle or nearby area; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-4 | Guardian issues an unintended mitigation request because it counts a duplicate as a new sample (F-3), accepts a high out-of-range value (F-6), or treats a high spike as valid/critical (F-8). | OS-1: Occupied vehicle moving in road traffic | The unintended request changes vehicle response or interrupts propulsion, preventing the driver from maintaining a safe trajectory and creating a collision risk for occupants or other road users. | S3 | - | - | ASIL-D |
| HE-5 | Same unintended-mitigation effects and conditions as HE-4 | OS-2: Occupied vehicle manoeuvring near other vehicles or pedestrians | The unintended request changes vehicle response during a manoeuvre, creating a collision risk for occupants or nearby road users. | S3 | - | - | ASIL-D |
| HE-6 | Temperature drifts upwards (F-7), temperature is outside upper limit of defined intervale (F-6), temperature unplausible spike appears (F-8) | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | False positive warning is issued, driver get's distracted and risk of collision with other vehicles rises | S3 | - | - | ASIL-D |  

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
for existing Guardian FSR wording, parameter values, priority, and status. Items
marked **Missing** or **Partial** are not satisfied merely by appearing in this
HARA; reconcile them into the Safety Concept before claiming requirement coverage.

| ID | HARA trace | Derived functional requirement | Safety Concept mapping | Coverage assessment |
|---|---|---|---|---|
| DFR-1 | SG-1; HE-1 to HE-3 | The Guardian shall detect each approved thermal-risk criterion and issue the corresponding occupant warning and mitigation response within its approved (`T_react = 100ms`). | FSR-1.1 to FSR-1.4 cover threshold, critical, trend, and hot-spot detection; FSR-1.6 and FSR-1.7 cover mitigation state/failure behavior. | **Partial Missing:** safety concept does not take `T_react` into account. |
| DFR-2 | SG-2; HE-1 to HE-3 | The Guardian shall monitor data freshness and validity, enter DEGRADED and report a fault when data connection or data is lost. | FSR-2.1 to FSR-2.6, FSR-2.8, FSR-3.4, FSR-3.5, and FSR-3.7. | **Partial:** input-side loss/invalidity behavior is covered. Guardian termination/hang is not occupant-protected by these FSRs; see DFR-5. |
| DFR-3 | SG-2, SG-4; HE-1 to HE-3 | Invalid or degraded input shall not lower or clear an active thermal warning. Thermal state may be lowered only after the defined recovery conditions are met using valid samples. | FSR-1.5, FSR-2.5, FSR-2.6, and FSR-3.6. | **Covered at Guardian behavior level**, subject to testing the stated valid-sample and recovery conditions. |
| DFR-4 | SG-3; HE-4, HE-5 | An invalid, stale, duplicated, or out-of-order sample shall not by itself cause CRITICAL state or a mitigation request. | FSR-1.2, FSR-3.2, FSR-3.3, and FSR-3.6. | **Partial / Missing:** these FSRs limit invalid-input state escalation, but no FSR explicitly states the output invariant that mitigation is issued only for a valid critical condition. Add that invariant to the Safety Concept and test it. |
| DFR-5 | SG-1, SG-2; HE-1 to HE-3; F-10 | An independent in-vehicle supervisor shall detect Guardian termination or loss of evaluation progress and request the defined monitoring-unavailable occupant warning through a path that does not depend on the Guardian or Evidence Collector. | FSR-2.7 only requires heartbeat observation by the Evidence Collector and restart of a terminated Guardian. | **Partial:** the [watchdog](../../watchdog/README.md) is a separate process that detects termination and hang (heartbeat from the evaluation loop, `T_hb` = 1500 ms) and reports `BTG_GuardianHeartbeatLoss` to DFM/OpenSOVD. **Missing:** the occupant warning; the watchdog does not request `DRIVER_WARNING_MONITORING_UNAVAILABLE`. |
| DFR-6 | Diagnostic goal; all faulted events | Each detected fault shall be traceable from Guardian/equipment event through DFM and OpenSOVD to the campaign verdict; diagnostic failures shall not delay safety reactions. | FSR-D.1 to FSR-D.4; EC-1 to EC-3. | **Covered for Guardian faults and campaign evidence. Missing allocation:** diagnostic reporting for the proposed independent supervisor in DFR-5 is not specified. |

### Safety Concept contradictions and missing requirements

- **Guardian failure response (DFR-5):** HARA SG-2 requires an independent
	in-vehicle response. Safety Concept FSR-2.7 only records a missing heartbeat
	in the Evidence Collector and restarts a terminated process. These are
	observability/recovery actions, not occupant protection; the safety response
	and hang detection are missing.
- **Mitigation gating (DFR-4):** FSR-3.2 and FSR-3.3 intentionally permit
	WARNING for high out-of-range or implausibly fast samples, since they may
	indicate a real thermal event. This is not a contradiction if WARNING is
	distinct from CRITICAL/mitigation. The missing requirement is an explicit
	rule that invalid data alone cannot trigger CRITICAL or mitigation.
- **Warning lead time (DFR-1):** FSR reaction budgets measure response after a
	criterion is observable. They do not establish that occupants are warned a
	defined time before a real battery event becomes dangerous. That requirement
	and its thermal validation basis are missing.
- **Goal-set mismatch:** this HARA defines SG-4, while the Safety Concept defines
	only SG-1 to SG-3. The mappings above are provisional until SG-4 and its
	allocation are reconciled there.
- **ASIL input inconsistency:** the hazardous-event table currently assigns
	ASIL-D while E and C are shown as `-`. ASIL cannot be derived from S alone;
	record justified E/C values and the applicable classification basis before
	treating those ASIL-D entries as assessed ratings.

## HARA-derived test scenarios

The scenarios below verify DFR-1 through DFR-6 and their mapped Safety Concept
requirements. Run each fault variant independently from a fresh Guardian instance
unless a scenario explicitly tests recovery. Record the active configuration and
observe the same input stream the Guardian receives. Timing parameters refer to
the approved Safety Concept configuration.

Scenarios that depend on a DFR marked **Missing** or **Partial** are requirement
gap checks, not evidence that the requirement is satisfied. Until the requirement
is approved and implemented, record the result as blocked or failed against the
safety objective; do not convert missing behavior into a passing test.

### Reviewed

#### Template

**HARA trace:** 

**Preconditions:**

**Stimulus:**

**Expected Result:**

**Evidence and Verdict Focus:**

### TS-01 Baseline

**HARA trace:** DRF-1 - DRF-4

**Preconditions:** Start the system

**Stimulus:** Publish valid, in-range, steadily updated temperatures below warning thresholds with valid quality (0x80) and monotonic increasing counter.

**Expected Result:** Monitoring becomes `OK` and thermal state becomes `MONITORING`. No warning, mitigation, or fault is emitted.

**Evidence and Verdict Focus**: Capture input/output stream, fail on unexpected fault or warning

### TS-02

**HARA trace:** HE-1 to HE-3; SG-1; DFR-1.

**Preconditions:** Start the Guardian with a valid temperature stream and
establish `MONITORING` below `θ_warn`.

**Stimulus:** Replay a valid rising-temperature profile through `θ_warn` and
then `θ_crit`.

**Expected Result:** The Guardian enters `WARNING` at `θ_warn`, then enters
`CRITICAL` and emits `DRIVER_WARNING_OVERTEMP` at `θ_crit`. Each response meets
the configured `T_react` budget.

**Evidence and Verdict Focus:** Record the threshold-crossing samples, state
transitions, warning event, and input-to-output latency. This verifies threshold
response.

### TS-03

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the Battery Thermal Guardian without starting the CAN
temperature source.

**Stimulus:** Publish no temperature data and allow the startup timeout to
expire.

**Expected Result:** The Guardian enters `DEGRADED` and reports the corresponding
startup/connection-loss fault within the configured timeout `T_stale` and reaction budget `T_react`.

**Evidence and Verdict Focus:** Record Guardian startup time, confirm no samples
reached its input, and capture the status transition, fault event, DFM record,
and latency.

### TS-04

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source and Guardian. Publish valid, in-range,
steadily updated data for at least `3 sec`.

**Stimulus:** Shut down the CAN source while leaving the Guardian running.

**Expected Result:** After no fresh sample arrives for `T_stale`, the Guardian
enters `DEGRADED` and reports the corresponding freshness fault within the
configured reaction budget.

**Evidence and Verdict Focus:** Record the last valid sample, source shutdown,
Guardian-input timeout, Guardian status/fault event, DFM record, and measured
latency.

### TS-05

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source, Data Broker, VSS Publisher, Guardian, OpenSOVD
and Evidence Collector tap points. Verify data is visible at the source and
publisher output and reaches the Guardian.

**Stimulus:** Delay or withhold an expected update so no fresh data reaches the
Guardian for longer than `T_stale`.

**Expected Result:** The Guardian enters `DEGRADED` and reports a freshness fault
within the configured reaction budget. A constant delay with regularly arriving
samples is not expected to be detected unless synchronized-clock age checking is
enabled.

**Evidence and Verdict Focus:** Record source timestamps and arrival times, the
Guardian-input gap, `T_stale`, status/fault event, DFM record, and latency.
Confirm the delayed stimulus reached the Guardian input.

### TS-06

**HARA trace:** HE-1 to HE-3; SG-2; DFR-2.

**Preconditions:** Start the CAN source, Data Broker, VSS Publisher, Guardian, OpenSOVD
and Evidence Collector tap points. Verify data is visible at the source and
publisher output and reaches the Guardian.

**Stimulus:** Inject a publisher/transport dropout between publisher output and
the Guardian input while keeping the source and publisher output active.

**Expected Result:** The Guardian enters `DEGRADED` and reports a freshness fault
after `T_stale`, within the configured reaction budget.

**Evidence and Verdict Focus:** Compare source, publisher-output, and
Guardian-input observations. Capture the Guardian event and DFM record; use the
tap-point differences to attribute the dropout to the publisher-to-Guardian
path. Mark attribution inconclusive if the observations cannot distinguish it
from source loss.

---

### To be reviewed

| ID | HARA trace | Preconditions and Stimulus | Expected result | Evidence and verdict focus |
|---|---|---|---|---|
| TS-04 | HE-1 to HE-3; SG-2; DFR-2, DFR-6; FSR-2.2, FSR-3.7, EC-2 | In separate runs, inject a duplicate sample and an out-of-order sample; optionally continue each fault until freshness timeout. | Non-fresh samples are ignored and do not advance thermal assessment. Persistent loss of fresh samples causes DEGRADED. The collector identifies sequence anomalies. | Capture timestamps, alive counter, publisher sequence, Guardian input/state, diagnostic trace, and collector attribution. |
| TS-05 | HE-1 to HE-3; SG-2; DFR-2, DFR-6; FSR-2.4 | Hold the maximum-temperature value constant while average or minimum changes by at least `Δ_stuck`; repeat with fast and slow heating profiles. | After both stuck criteria hold, enter DEGRADED and report a signal-stuck fault within the specified hidden-error bound. | Record all three signals, detection error, and diagnostic trace. Include slow nominal heating as a negative test. |
| TS-06 | HE-1 to HE-3; SG-2; DFR-2; F-1 limitation probe | Keep all temperature values constant while timestamps and source alive counter continue to advance. | Characterize whether the current design can distinguish a genuinely constant battery from a fully frozen signal. Do not claim detection unless an implemented mechanism detects it. | Record as a limitation/coverage result, not a passing FSR-2.4 test; the current stuck-maximum check requires another temperature channel to move. |
| TS-07 | HE-1 to HE-3; SG-2, SG-4; DFR-3; FSR-1.5, FSR-2.5, FSR-2.6, FSR-3.6 | First reach WARNING (repeat at CRITICAL). Inject a low out-of-range or invalid-quality sample, then restore valid samples. | Invalid input does not lower or clear the active thermal state. Lowering/recovery occurs only under valid-sample, hysteresis, and recovery conditions. | Capture validity, state before/after injection, monitoring status, recovery count, and de-escalation point. |
| TS-08 | HE-1, HE-4, HE-5; SG-1, SG-3; DFR-1, DFR-4; FSR-3.2, FSR-3.3, FSR-3.6 | Separately inject a high out-of-range sample and a one-sample rise exceeding `r_max` while below CRITICAL. | Raise at least WARNING because the sample may indicate real danger; do not enter CRITICAL or request mitigation from the invalid sample alone. | Verify state, warning event, absence of mitigation, fault record, and latency. Do not label the expected WARNING a false positive solely because input was anomalous. |
| TS-09 | HE-4, HE-5; SG-3; DFR-4 | While below CRITICAL, inject duplicate, high out-of-range, and high-spike variants separately; then provide valid critical input as a positive control. | Invalid/repeated input alone causes no mitigation. Valid critical input produces the defined mitigation request. | This directly tests DFR-4, for which no explicit FSR exists. Record as a requirement gap until the mitigation-gating invariant is added to the Safety Concept. |
| TS-10 | HE-6; SG-4; | Present the approved warning for uncertain high-temperature data in an OS-1 driving simulator/human-factors setup, with a credible thermal-warning positive control. | Communicate uncertainty and approved driver action without suppressing a potentially valid thermal warning or inducing the defined unsafe response. | Blocked until HMI acceptance criteria, warning design, and owner are approved. A Guardian replay cannot verify driver response. |
| TS-11 | Diagnostic goal; DFR-6; FSR-D.1 to FSR-D.4, EC-1 to EC-3 | Run a fault scenario that causes a Guardian diagnostic; separately delay the DFM write or suppress OpenSOVD visibility. | Safety behavior is not delayed by diagnostics. The collector correlates records or flags missing/late evidence. | Correlate run ID, fault code, detecting requirement, event time, DFM/OpenSOVD record, and verdict. Diagnostics from a future independent supervisor remain uncovered. |
| TS-12 | HE-1 to HE-3; SG-1, SG-2; F-10; DFR-5 | With valid temperature input active, terminate the Guardian process. | Proposed independent supervisor requests `DRIVER_WARNING_MONITORING_UNAVAILABLE`; runtime may restart the process. The collector log is not the occupant response. | Warning behavior is **not implemented**: mark blocked or failed against the safety objective, never PASS from heartbeat evidence alone. Detection is implemented (watchdog); manual run: SIGKILL reported 1.4 s after the kill, Docker restart, Passed on the new Guardian's first heartbeat 0.7 s later. Measure warning latency after implementation. |
| TS-13 | HE-1 to HE-3; SG-1, SG-2; F-10; DFR-5 | With valid input active, pause the Guardian evaluation loop while leaving its process alive. | Progress-aware monitor detects the missing evaluation progress and requests monitoring-unavailable warning; hang recovery is separately defined. | **Partial:** the heartbeat is sent from the evaluation loop, so a stalled loop stops it; manual run with `docker pause`: reported 1.8 s after the pause, Passed 0.4 s after unpause. FSR-2.7 still does not require evaluation progress, and hang recovery (restart of a hung Guardian) and the warning are missing. |
| TS-14 | HE-1 to HE-3; SG-1; DFR-1 | Replay or simulate an approved battery thermal profile from warning trigger through the defined dangerous condition. | Occupants receive warning at least the approved lead time before the dangerous condition. | **Blocked/missing:** no approved lead-time value, reference condition, or validated thermal profile exists. `T_react` alone cannot pass this test. |
| TS-15 | SG-2; DFR-5, DFR-6 | After an independent supervisor is implemented, terminate or hang the Guardian and observe the supervisor's diagnostic output. | The supervisor's fault is traceable through diagnostics to the campaign verdict without delaying its warning response. | **Missing allocation:** no Safety Concept FSR/EC currently defines supervisor diagnostics. The watchdog already reports `BTG_GuardianHeartbeatLoss` with session IDs to DFM/OpenSOVD, but no campaign verdict uses it. Add and approve that allocation before claiming coverage. |

For each run, record the operational situation, fault variant and injection
parameters, run/correlation IDs, input and output timestamps, expected and
observed reaction, diagnostic visibility, and PASS/FAIL/INCONCLUSIVE verdict.
PASS requires the fault to reach the Guardian input, all required reactions to
meet their budgets, no forbidden reaction, and complete evidence. Use
INCONCLUSIVE when the stimulus did not reach the Guardian or the required vehicle
interface/evidence was unavailable. Retain failed runs in the report.

These scenarios verify the stated software and evidence behaviors; they do not establish vehicle-level S/E/C ratings or prove occupant safety by themselves.
Mark a scenario PASS only when its requirement is approved, the stimulus reaches the intended boundary, all specified reactions occur within budget, no forbidden reaction occurs, and required evidence is complete. Keep blocked and failed scenarios visible in the report.

## AI Assistance

This document was revised with the assistance of **GitHub Copilot (GPT-6 Luna)**.

The watchdog coverage of DFR-5, TS-12, TS-13 and TS-15 was updated with the
assistance of **Claude Code** using the model **Claude Opus 5.5**
(`claude-opus-5-5`).
