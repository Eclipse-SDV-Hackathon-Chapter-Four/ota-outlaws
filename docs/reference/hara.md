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


## Derived implementation requirements

This crosswalk links the HARA goals to implementation behavior and verification.
The detailed Guardian FSRs, priorities, status, and parameters are maintained in
the [Safety Concept](../explanation/safety-concept.md#functional-safety-requirements),
which remains the detailed requirement source. Approve parameter values against
the vehicle/system safety concept before using them as a safety baseline.

| Requirement ID | HARA trace | Requirement and allocation | Verification |
|---|---|---|---|
| FSR-1.1, FSR-1.2 | SG-1; HE-1 to HE-3 | On valid data meeting `θ_warn`, the Guardian shall enter WARNING or a more severe state within `T_react`. On valid data meeting `θ_crit`, it shall enter CRITICAL and publish `DRIVER_WARNING_OVERTEMP` within `T_react`. | Replay heating profiles through both thresholds; assert state, event, and latency. |
| FSR-1.3, FSR-1.4 | SG-1; HE-1 to HE-3 | When valid samples meet the configured rise-rate criterion for `T_trend`, or the maximum-to-average difference exceeds `Δ_hotspot`, the Guardian shall enter WARNING or a more severe state within the specified reaction budget. | Replay a sustained fast-rise profile below `θ_warn` and a localized hot-spot profile; assert state and latency. |
| FSR-2.1 to FSR-2.4 | SG-2; HE-1 to HE-3 | The Guardian shall detect startup timeout, stale input, stopped source counters, and a frozen maximum signal according to the specified timing and value criteria; it shall report the corresponding monitoring status and fault. | Test no-source startup, outage, source dropout, repeated counter, and frozen-maximum cases, including slow nominal heating as a negative test. |
| FSR-2.7 | SG-2; HE-1 to HE-3; F-10 | The Guardian shall publish a progress heartbeat after each completed safety-evaluation cycle. The Evidence Collector records missing heartbeats for campaigns; runtime restart is recovery behavior, not the occupant safety response. | Terminate the process and hang the evaluation loop in separate runs; verify progress heartbeat stops and collect the external monitor/runtime response. |
| VSR-2.1 (proposed) | SG-1, SG-2; HE-1 to HE-3; F-10 | An independent in-vehicle supervisor shall detect a missing Guardian progress heartbeat within `T_hb` and request `DRIVER_WARNING_MONITORING_UNAVAILABLE` over a path independent of the Guardian and Evidence Collector. Owner: vehicle supervisor/HMI; not currently implemented. | Kill and hang the Guardian while temperature input is active; verify warning request within the approved response budget and separately verify runtime restart of a terminated process. |
| FSR-2.5, FSR-2.6 | SG-2 and SG-4; HE-1 to HE-3 | While monitoring is DEGRADED, the Guardian shall not lower the thermal state. Monitoring shall return to OK only after `N_recover` consecutive valid samples, after which thermal state is reassessed from fresh data. | Raise a warning, inject a monitoring fault and a low invalid sample, and verify the state is retained; restore valid samples and verify specified recovery behavior. |
| FSR-1.5, FSR-3.6 | SG-4; HE-1 | The Guardian shall not lower an active thermal state in response to invalid input. It shall lower the state only after the triggering criterion is undercut by `θ_hyst` for `N_recover` consecutive valid samples and monitoring is not DEGRADED. | Start in WARNING and CRITICAL; inject one low out-of-range, invalid-quality, or implausible sample and verify no state decrease. Then provide the required valid recovery sequence and verify de-escalation. |
| FSR-3.2, FSR-3.3, FSR-3.6 | SG-1, SG-3; HE-1, HE-4, HE-5, HE-6 | Invalid high readings shall not alone raise the thermal state to CRITICAL, but high out-of-range values or implausibly fast rises shall still raise at least WARNING because they may indicate real danger. Invalid input shall not lower the state. | Inject high/low out-of-range values and spikes at multiple states; verify WARNING behavior for potentially dangerous high values, no invalid-only CRITICAL/mitigation, and no lowering from invalid input. |
| FSR-1.2, FSR-3.2, FSR-3.3, FSR-3.6 | SG-3; HE-4, HE-5 | Mitigation publication shall be gated by the defined CRITICAL condition; invalid input alone shall not cause CRITICAL. This mapping assumes the Guardian publishes mitigation only on entering CRITICAL; confirm that contract in the implementation. | Inject duplicate, high out-of-range, and spike samples below CRITICAL; verify none alone causes a mitigation request. Verify valid critical input still causes the specified request. |
| VSR-HMI-01 (proposed) | SG-4; HE-6 | The vehicle warning function shall communicate possible thermal danger and the approved driver action for uncertain high-temperature input without prompting an unsafe abrupt response. Owner: vehicle warning/HMI, outside the Guardian unless the item boundary is expanded. | Define acceptance criteria with the safety/HMI owner and validate the warning in OS-1 using an approved human-factors method. |
| FSR-D.1, FSR-D.2 | Diagnostic goal; all faulted events | Every Guardian-reported fault shall be written to DFM with a code identifying the detecting requirement. A delayed or failed write shall not delay the safety reaction. Expected records shall become visible through OpenSOVD within `T_diag`. | Correlate Guardian events and DFM records; inject delayed writes and missing OpenSOVD visibility. |
| EC-1 to EC-3 | Campaign evidence; F-2 to F-5, F-9 | The Evidence Collector shall attribute source/publisher/transport loss, identify duplicate, reordered, and missing samples by sequence number, and measure transport delay where observable. These are evidence requirements, not Guardian safety reactions. | Inject source, publisher, transport, duplicate, reorder, drop, and constant-delay scenarios; compare collector output with observed tap points. |

**Gaps and conflicts to resolve:**

- SG-4 is new to this HARA; the linked Safety Concept has only three safety
	goals. Reconcile SG-4 and its hazardous-event mapping there before treating
	these documents as one approved baseline.
- The Safety Concept intentionally raises WARNING for high out-of-range values
	and implausibly fast rises. Do not interpret every such warning as a false
	positive or suppress it; the input may represent a real thermal event. HE-6
	needs an independently justified driver-harm path, and its mitigation belongs
	to the vehicle warning/HMI function, not automatically to the Guardian.
- The current in-vehicle design has no independent Guardian monitor: the Evidence
	Collector is test infrastructure and runtime restart alone does not warn
	occupants. VSR-2.1 is proposed and remains an open safety dependency until an
	owner, independent warning path, timing budget, and verification are approved.
- F-4 is now defined as an upstream publisher/transport omission. A sample lost
	inside Guardian processing is a separate application failure and must not be
	mislabeled as transport loss.

## HARA-derived test scenarios

Run each fault variant independently from a fresh Guardian instance unless a scenario explicitly tests recovery. Begin with valid nominal input, record the active configuration and signal profile, and observe the same input stream the
Guardian receives. `T_react`, `T_stale`, thresholds, and other parameters refer to the approved Safety Concept configuration. The vehicle situations provide HARA context; a software replay alone does not demonstrate driver or occupant behavior in that situation.

| ID | HARA trace | Preconditions and stimulus | Expected result | Evidence and verdict focus |
|---|---|---|---|---|
| TS-01 | Baseline; SG-1 to SG-3 | Start a fresh Guardian and publish valid, in-range, steadily updated temperatures below warning thresholds. | Monitoring becomes OK and thermal state becomes MONITORING; no warning, mitigation, or fault is emitted. | Capture input/output stream and heartbeat. Fail on unexpected fault or warning. |
| TS-02 | HE-1 to HE-3; SG-1; FSR-1.1, FSR-1.2 | Replay a valid rising-temperature profile through `θ_warn` and then `θ_crit`. | Reach WARNING at `θ_warn`; reach CRITICAL and emit `DRIVER_WARNING_OVERTEMP` at `θ_crit`, each within its configured reaction budget. | Measure input-to-state/event latency at the Evidence Collector. This verifies threshold response, not real-pack warning lead time; that needs an approved thermal-event profile and deadline. |
| TS-03 | HE-1 to HE-3; SG-2; FSR-2.1, FSR-2.2 | In separate runs, start with no source, stop updates, delay updates beyond `T_stale`, or inject a source/publisher dropout at the Guardian input. | Enter DEGRADED and report the corresponding startup/freshness fault within the specified budget. Do not lower an existing thermal state. | Record exact tap-point fault onset, Guardian status/event, DFM record, and source-versus-transport attribution. Resolve F-4 ownership before using an omission variant. |
| TS-04 | HE-1 to HE-3; SG-2; FSR-2.2, FSR-3.7, EC-2 | In separate runs, inject a duplicate sample and an out-of-order sample; optionally continue each fault until freshness timeout. | Non-fresh samples are ignored and do not advance the thermal assessment. Persistent loss of fresh samples causes DEGRADED as specified. The collector identifies sequence anomalies. | Capture source timestamps, alive counter, publisher sequence, Guardian input, state changes, and collector attribution. |
| TS-05 | HE-1 to HE-3; SG-2; FSR-2.4 | Hold the maximum-temperature value constant while average or minimum changes by at least `Δ_stuck`; repeat with fast and slow heating profiles. | After both stuck criteria hold, enter DEGRADED and report a signal-stuck fault within the specified hidden-error bound. | Record all three signals and calculate detection error against `Δ_stuck` plus one CAN step. Include slow nominal heating as a negative test. |
| TS-06 | HE-1 to HE-3; SG-2; F-1 limitation probe | Keep all temperature values constant while timestamps and source alive counter continue to advance. | Characterize whether the current design can distinguish a genuinely constant battery from a fully frozen signal. Do not claim detection unless an independent plausibility mechanism detects it. | Record as a limitation/coverage result, not a passing FSR-2.4 test; the current stuck-maximum requirement needs another temperature channel to move. |
| TS-07 | HE-1 to HE-3; SG-4; FSR-1.5, FSR-2.5, FSR-3.2, FSR-3.4, FSR-3.6 | First reach WARNING (repeat at CRITICAL). In separate runs, inject one low out-of-range sample and one sample with invalid quality. Then restore valid samples. | The invalid sample is rejected and shall not lower or clear the active thermal state. Any transition out of DEGRADED or reduction in thermal state follows the specified valid-sample, hysteresis, and recovery rules. | Capture sample validity, state before/after injection, monitoring status, and recovery count. Verify a single invalid sample cannot clear an alert. |
| TS-08 | HE-1, HE-4, HE-5; SG-1, SG-3; FSR-3.2, FSR-3.3, FSR-3.6 | In separate runs, inject a high out-of-range sample and a one-sample rise exceeding `r_max`, while below CRITICAL. | Raise at least WARNING because the sample may indicate real danger, but do not enter CRITICAL or request mitigation from the invalid sample alone. | Verify state, warning event, absence of mitigation, fault record, and reaction latency. Do not label the expected warning a false positive solely because the injected sample was invalid. |
| TS-09 | HE-4, HE-5; SG-3 | While below CRITICAL, inject duplicate, high out-of-range, and high-spike variants individually. Separately provide valid critical input as a positive control. | Invalid/repeated input alone does not cause unintended mitigation. Valid critical input produces the defined critical/mitigation behavior. | Capture input validity, state transitions, and mitigation events. Run in OS-1/OS-2 test setups only if the mitigation consumer and vehicle-response interface are available. |
| TS-10 | HE-6; SG-4; proposed VSR-HMI-01 | Present the approved warning for an uncertain high-temperature condition in an OS-1 driving simulator or other safety-approved human-factors setup. Include a credible thermal-warning positive control. | The warning communicates the approved action without inducing the defined unsafe driver response, while the credible thermal warning remains salient and actionable. | Requires owner-approved human-factors acceptance criteria and a representative HMI. A Guardian unit/replay test cannot establish this result. |
| TS-11 | Diagnostic goal; FSR-D.1, FSR-D.2 | Run a fault scenario that causes a Guardian diagnostic; separately delay the DFM write or suppress OpenSOVD visibility. | Safety reaction is not delayed by diagnostics. The collector reports the matching DFM/OpenSOVD record or flags its lateness/absence. | Correlate run ID, fault code, detecting requirement, event time, DFM record, OpenSOVD visibility, and collector verdict. |
| TS-12 | HE-1 to HE-3; SG-1, SG-2; F-10; FSR-2.7, VSR-2.1 | With valid temperature input active, terminate the Guardian process. | The independent vehicle supervisor requests `DRIVER_WARNING_MONITORING_UNAVAILABLE` after the heartbeat timeout; runtime restarts the terminated process according to its configured restart policy. The Evidence Collector records both actions but is not the safety response. | Measure heartbeat loss to warning-request latency and process recovery time; verify the warning path does not depend on Guardian or collector. |
| TS-13 | HE-1 to HE-3; SG-1, SG-2; F-10; FSR-2.7, VSR-2.1 | With valid temperature input active, block or pause the Guardian's safety-evaluation loop while leaving the process alive. | A progress heartbeat stops; the independent vehicle supervisor requests `DRIVER_WARNING_MONITORING_UNAVAILABLE`. Recovery behavior for a hung process must be separately defined and verified. | Confirm heartbeat is coupled to evaluation progress, not merely process existence. If the monitor cannot detect the hang, mark the scenario FAIL or INCONCLUSIVE per its approved verdict rule, not PASS based only on collector logs. |

For each run, record the operational situation, fault variant and injection
parameters, run/correlation IDs, input and output timestamps, expected and
observed reaction, diagnostic visibility, and PASS/FAIL/INCONCLUSIVE verdict.
PASS requires the fault to reach the Guardian input, all required reactions to
meet their budgets, no forbidden reaction, and complete evidence. Use
INCONCLUSIVE when the stimulus did not reach the Guardian or the required vehicle
interface/evidence was unavailable. Retain failed runs in the report.

These scenarios verify the stated software and evidence behaviors; they do not
establish vehicle-level S/E/C ratings or prove occupant safety by themselves.

## AI Assistance

This document was revised with the assistance of **GitHub Copilot (GPT-6 Luna)**.
