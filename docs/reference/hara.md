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
| F-11 | CAN source sets the quality flag to `INVALID` or `ERROR_NOT_AVAILABLE` on a fresh temperature frame | Source/Signal | Guardian fails to reject the sample, mark monitoring unavailable, or report the quality fault; invalid temperature data may be treated as trustworthy or its loss may go unnoticed |

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
| HE-1 | Guardian misses or delays detection because it accepts stale samples (F-1), samples arriving after the warning deadline (F-2), accepts duplicated samples (F-3), continues using data after an upstream omitted update (F-4), derives a misleading trend from reordered samples (F-5), accepts a low out-of-range value (F-6), accepts a downward temperature drift or spike (F-7, F-8), fails to detect source loss (F-9), stops evaluating and publishing safety events after process termination or hang (F-10), or treats a fresh sample with invalid source quality as trustworthy or fails to report monitoring loss (F-11). | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | Battery fire or smoke develops before occupants receive a usable warning and can stop in a safe place or evacuate; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-2 | Same missed/delayed-detection effects and fault conditions as HE-1, including invalid source quality (F-11) and Guardian termination or hang (F-10) | OS-2: Occupied vehicle manoeuvring at low speed near other vehicles or pedestrians | Battery fire or smoke develops before occupants can safely stop or move away; occupants or nearby people may be exposed to smoke or heat while the vehicle is manoeuvring. | S3 | - | - | ASIL-D |
| HE-3 | Same missed/delayed-detection effects and fault conditions as HE-1, including invalid source quality (F-11) and Guardian termination or hang (F-10) | OS-3: Vehicle parked or charging with occupants in or immediately beside it | Battery fire or smoke develops before occupants receive a usable warning and can leave the vehicle or nearby area; occupants may be exposed to smoke or heat. | S3 | - | - | ASIL-D |
| HE-4 | Guardian issues an unintended mitigation request because it counts a duplicate as a new sample (F-3), accepts a high out-of-range value (F-6), or treats a high spike as valid/critical (F-8). | OS-1: Occupied vehicle moving in road traffic | The unintended request changes vehicle response or interrupts propulsion, preventing the driver from maintaining a safe trajectory and creating a collision risk for occupants or other road users. | S3 | - | - | ASIL-D |
| HE-5 | Same unintended-mitigation effects and conditions as HE-4 | OS-2: Occupied vehicle manoeuvring near other vehicles or pedestrians | The unintended request changes vehicle response during a manoeuvre, creating a collision risk for occupants or nearby road users. | S3 | - | - | ASIL-D |
| HE-6 | Temperature drifts upwards (F-7), temperature is outside upper limit of defined intervale (F-6), temperature unplausible spike appears (F-8) | OS-1: Occupied vehicle moving in road traffic, with limited opportunity to stop immediately | False positive warning is issued, driver get's distracted and risk of collision with other vehicles rises | S3 | - | - | ASIL-D |  

F-11 maps to HE-1 through HE-3 because it can make thermal monitoring
unavailable during the same hazardous vehicle situations; it does not create a
separate hazardous event. The table carries forward the existing S3 entries for
those events. E and C remain unassessed, so the displayed ASIL-D values are not
fully derived or confirmed for F-11 until exposure and controllability are
supported for each operating situation.

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
| DFR-2 | SG-2; HE-1 to HE-3; F-11 | The Guardian shall treat a fresh sample whose CAN quality flag is not `VALID` as unusable, enter `DEGRADED`, and report a quality fault within `T_react`; it shall not treat the sample as evidence that the battery is safe. | FSR-3.4; general loss handling in FSR-2.1 to FSR-2.6. | **Specified/implemented in the Safety Concept** for the Guardian response. Vehicle-level warning independence remains the separate DFR-5 gap. |
| DFR-3 | SG-2, SG-4; HE-1 to HE-3; F-11 | Invalid or degraded input shall not lower or clear an active thermal warning. Thermal state may be lowered only after the defined recovery conditions are met using valid samples. | FSR-1.5, FSR-2.5, FSR-2.6, and FSR-3.6. | **Covered at Guardian behavior level**, subject to testing the stated valid-sample and recovery conditions. |
| DFR-4 | SG-3; HE-4, HE-5 | An invalid, stale, duplicated, or out-of-order sample shall not by itself cause CRITICAL state or a mitigation request. | FSR-1.2, FSR-3.2, FSR-3.3, and FSR-3.6. | **Partial / Missing:** these FSRs limit invalid-input state escalation, but no FSR explicitly states the output invariant that mitigation is issued only for a valid critical condition. Add that invariant to the Safety Concept and test it. |
| DFR-5 | SG-1, SG-2; HE-1 to HE-3; F-10 | An independent in-vehicle supervisor shall detect Guardian termination or loss of evaluation progress and request the defined monitoring-unavailable occupant warning through a path that does not depend on the Guardian or Evidence Collector. | FSR-2.7 only requires heartbeat observation by the Evidence Collector and restart of a terminated Guardian. | **Partial:** the [watchdog](../../watchdog/README.md) is a separate process that detects termination and hang (heartbeat from the evaluation loop, `T_hb` = 1500 ms) and reports `BTG_GuardianHeartbeatLoss` to DFM/OpenSOVD. **Missing:** the occupant warning; the watchdog does not request `DRIVER_WARNING_MONITORING_UNAVAILABLE`. |
| DFR-6 | Diagnostic goal; all faulted events | Each detected fault shall be traceable from Guardian/equipment event through DFM and OpenSOVD to the campaign verdict; diagnostic failures shall not delay safety reactions. | FSR-D.1 to FSR-D.4; EC-1 to EC-3. | **Covered for Guardian faults and campaign evidence. Missing allocation:** diagnostic reporting for the proposed independent supervisor in DFR-5 is not specified. |
| DFR-7 | F-3 | After two messages with the same counter the monitoring state `SUSPECT` is reported, after 10 messages it switchs to `DEGRADED`| | **Missing** |
| DRF-8 | F-3 | After messages with same counter values are received and ten messages with monotonic increasing counter are received, signal state recovers to `OK`| | **Missing** |

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

### Test Template

**HARA trace:** 

**Preconditions:**

**Stimulus:**

**Expected Result:**

**Expected Mitigations:**

**Evidence and Verdict Focus:**

### TS-01 Baseline

**HARA trace:** DRF-1 - DRF-4

**Preconditions:** Start the system, subscribe to the mitigation topic

**Stimulus:** Publish valid, in-range, steadily updated temperatures below warning thresholds with valid quality (0x80) and monotonic increasing counter.

**Expected Result:** Initial thermal state is `CLEAR`. Monitoring becomes `OK` and thermal state becomes `MONITORING`. No warning, mitigation, or fault is emitted.

**Expected Mitigations:** None

**Evidence and Verdict Focus**: Capture input/output stream, fail on unexpected fault, warning or emitted mitigation.

### TS-02

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

### TS-03

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

### TS-04

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

### TS-05

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

### TS-06

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

### TS-07 Duplicate message

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
does not cause a freshness timeout. If more than two messages with the same 
counter are received, the Guardian reports `SUSPECT` monitoring state. 
After ten messages with the same counter, the Guardian reports `DEGRADED`.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`

**Evidence and Verdict Focus:** Capture publisher sequence, source timestamp,
alive counter, Guardian-input arrival order, Guardian state/fault events, DFM
record, and collector attribution. A duplicate not visible at the Guardian-
input tap is an inconclusive injection, not a pass.

### TS-08 Out-of-order message

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

### TS-09 Stuck maximum temperature

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

### TS-10 All temperature values frozen probe

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

### TS-11

**HARA trace:** HE-1 to HE-3; SG-2 and SG-4; F-6 (low out-of-range value);
DFR-2 and DFR-3; FSR-1.5, FSR-2.5, FSR-2.6, FSR-3.2, and FSR-3.6.

**Preconditions:** Start a fresh Guardian with valid input. First establish
`WARNING` and signal quality `DEGRADED`.

**Stimulus:** Inject one low out-of-range temperature (Below 10 °C) sample, then restore
valid samples satisfying the configured recovery conditions.

**Expected Result:** Invalid input does not lower or clear the active thermal
state. A transition out of `DEGRADED` or a reduction in thermal state occurs
only after the required valid samples, hysteresis, and recovery conditions are
met.

**Expected Mitigations:** No new mitigation is triggered solely by the invalid
sample. Any previously active warning or critical response remains in effect
until the valid recovery conditions permit a state change.

**Evidence and Verdict Focus:** Record sample quality/range, state before and
after injection, monitoring status, recovery sample count, hysteresis threshold,
and the exact de-escalation point. Fail if a single invalid sample lowers or
clears the active thermal state.

### TS-12

**HARA trace:** HE-1 to HE-3; SG-2 and SG-4; F-6 (low out-of-range value);
DFR-2 and DFR-3; FSR-1.5, FSR-2.5, FSR-2.6, FSR-3.2, and FSR-3.6.

**Preconditions:** Start a fresh Guardian with valid input. First establish
`CRITICAL` and signal quality `DEGRADED`.

**Stimulus:** Inject one low out-of-range temperature (Below 10 °C) sample, then restore
valid samples satisfying the configured recovery conditions.

**Expected Result:** Invalid input does not lower or clear the active thermal
state. A transition out of `DEGRADED` or a reduction in thermal state occurs
only after the required valid samples, hysteresis, and recovery conditions are
met.

**Expected Mitigations:** No new mitigation is triggered solely by the invalid
sample. Any previously active warning or critical response remains in effect
until the valid recovery conditions permit a state change.

**Evidence and Verdict Focus:** Record sample quality/range, state before and
after injection, monitoring status, recovery sample count, hysteresis threshold,
and the exact de-escalation point. Fail if a single invalid sample lowers or
clears the active thermal state.

### TS-13

**HARA trace:** F-11; HE-1 to HE-3; SG-2 and SG-4; DFR-2 and DFR-3;
FSR-3.4 and FSR-3.6.

**Preconditions:** Start a fresh Guardian with valid samples and establish
`WARNING`; repeat from `CRITICAL` to verify thermal-state retention.

**Stimulus:** In a separate run, publish a fresh sample whose quality flag is
`INVALID` or `ERROR_NOT_AVAILABLE`. Then restore fresh valid samples.

**Expected Result:** The Guardian rejects the sample, sets monitoring status to
`DEGRADED`, and reports a quality fault within `T_react`. The thermal state is
not lowered by the invalid sample. Monitoring recovers only after the configured
consecutive-valid-sample rule; thermal-state reduction still follows hysteresis.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` while monitoring
is `DEGRADED`; no new overtemperature mitigation is caused by invalid quality
alone.

**Evidence and Verdict Focus:** Capture the quality flag, source timestamp,
alive counter, thermal state before/after, monitoring-status transition, fault
code, DFM record, recovery sample count, and response latency. Add a dedicated
quality-invalid fault ID to the inventory before claiming complete fault
traceability.

### TS-14

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

### To be reviewed

| ID | HARA trace | Preconditions and Stimulus | Expected result | Evidence and verdict focus |
|---|---|---|---|---|
| TS-11/12/13 | HE-1 to HE-3; SG-2, SG-4; DFR-3; FSR-1.5, FSR-2.5, FSR-2.6, FSR-3.6 | First reach WARNING (repeat at CRITICAL). Inject a low out-of-range or invalid-quality sample, then restore valid samples. | Invalid input does not lower or clear the active thermal state. Lowering/recovery occurs only under valid-sample, hysteresis, and recovery conditions. | Capture validity, state before/after injection, monitoring status, recovery count, and de-escalation point. |
| TS-14 | HE-1, HE-4, HE-5; SG-1, SG-3; DFR-1, DFR-4; FSR-3.2, FSR-3.3, FSR-3.6 | Separately inject a high out-of-range sample and a one-sample rise exceeding `r_max` while below CRITICAL. | Raise at least WARNING because the sample may indicate real danger; do not enter CRITICAL or request mitigation from the invalid sample alone. | Verify state, warning event, absence of mitigation, fault record, and latency. Do not label the expected WARNING a false positive solely because input was anomalous. |
| TS-15 | HE-4, HE-5; SG-3; DFR-4 | While below CRITICAL, inject duplicate, high out-of-range, and high-spike variants separately; then provide valid critical input as a positive control. | Invalid/repeated input alone causes no mitigation. Valid critical input produces the defined mitigation request. | This directly tests DFR-4, for which no explicit FSR exists. Record as a requirement gap until the mitigation-gating invariant is added to the Safety Concept. |
| TS-16 | HE-6; SG-4; | Present the approved warning for uncertain high-temperature data in an OS-1 driving simulator/human-factors setup, with a credible thermal-warning positive control. | Communicate uncertainty and approved driver action without suppressing a potentially valid thermal warning or inducing the defined unsafe response. | Blocked until HMI acceptance criteria, warning design, and owner are approved. A Guardian replay cannot verify driver response. |
| TS-17 | Diagnostic goal; DFR-6; FSR-D.1 to FSR-D.4, EC-1 to EC-3 | Run a fault scenario that causes a Guardian diagnostic; separately delay the DFM write or suppress OpenSOVD visibility. | Safety behavior is not delayed by diagnostics. The collector correlates records or flags missing/late evidence. | Correlate run ID, fault code, detecting requirement, event time, DFM/OpenSOVD record, and verdict. Diagnostics from a future independent supervisor remain uncovered. |
| TS-18 | HE-1 to HE-3; SG-1, SG-2; F-10; DFR-5 | With valid temperature input active, terminate the Guardian process. | Proposed independent supervisor requests `DRIVER_WARNING_MONITORING_UNAVAILABLE`; runtime may restart the process. The collector log is not the occupant response. | Warning behavior is **not implemented**: mark blocked or failed against the safety objective, never PASS from heartbeat evidence alone. Measure warning/recovery latency after implementation. |
| TS-19 | HE-1 to HE-3; SG-1, SG-2; F-10; DFR-5 | With valid input active, pause the Guardian evaluation loop while leaving its process alive. | Progress-aware monitor detects the missing evaluation progress and requests monitoring-unavailable warning; hang recovery is separately defined. | **Missing:** current FSR-2.7 does not require heartbeat to prove evaluation progress or specify hang recovery. Until defined and implemented, record the gap. |
| TS-20 | HE-1 to HE-3; SG-1; DFR-1 | Replay or simulate an approved battery thermal profile from warning trigger through the defined dangerous condition. | Occupants receive warning at least the approved lead time before the dangerous condition. | **Blocked/missing:** no approved lead-time value, reference condition, or validated thermal profile exists. `T_react` alone cannot pass this test. |
| TS-21 | SG-2; DFR-5, DFR-6 | After an independent supervisor is implemented, terminate or hang the Guardian and observe the supervisor's diagnostic output. | The supervisor's fault is traceable through diagnostics to the campaign verdict without delaying its warning response. | **Missing allocation:** no Safety Concept FSR/EC currently defines supervisor diagnostics. Add and approve that allocation before claiming coverage. |

For each run, record the operational situation, fault variant and injection
parameters, run/correlation IDs, input and output timestamps, expected and
observed reaction, diagnostic visibility, and PASS/FAIL/INCONCLUSIVE verdict.
PASS requires the fault to reach the Guardian input, all required reactions to
meet their budgets, no forbidden reaction, and complete evidence. Use
INCONCLUSIVE when the stimulus did not reach the Guardian or the required vehicle
interface/evidence was unavailable. Retain failed runs in the report.

These scenarios verify the stated software and evidence behaviors; they do not establish vehicle-level S/E/C ratings or prove occupant safety by themselves.
Mark a scenario PASS only when its requirement is approved, the stimulus reaches the intended boundary, all specified reactions occur within budget, no forbidden reaction occurs, and required evidence is complete. Keep blocked and failed scenarios visible in the report.

## Fault Catalog Single-Fault Test Cases

The cases below add one test per fault-catalog entry. A test injects only the
named fault; nominal input and recovery samples are setup/control data, not
additional injected faults. Isolated/repeated behaviors and high/low range
directions are separate cases. **Blocked** means a required threshold, response,
owner, or observable oracle is not defined or implemented; it is not a passing
result.

| Fault catalog entry | HARA test case |
|---|---|
| Overtemperature Warning | TS-22 |
| Overtemperature Critical | TS-23 |
| Undertemperature | TS-24 |
| FreshnessLost | TS-25 |
| CounterStuck | TS-26 |
| SignalStuck | TS-27 |
| QualityInvalid | TS-28 |
| OutOfRange | TS-30 (low), TS-31 (high) |
| RateImplausible | TS-32 |
| Fast-Heating Trend | TS-33 |
| Startup Without Source | TS-36 |
| Isolated vs. Repeated Spike | TS-37 (isolated), TS-38 (repeated) |
| Counter Error, Isolated vs. Repeated | TS-39 (isolated), TS-40 (repeated) |
| Heartbeat Loss | TS-41 (termination), TS-42 (hang) |
| Stale Timestamp | TS-43 |
| Upper-Scale Saturation | TS-47 |

### TS-22 Overtemperature Warning

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

### TS-23 Overtemperature Critical

**HARA trace:** HE-1 to HE-3; SG-1; catalog fault: Overtemperature Critical.

**Preconditions:** Start with valid input and thermal state `WARNING`; subscribe
to Guardian state, DTC, and mitigation outputs.

**Stimulus:** Raise only the valid maximum cell temperature to `θ_crit`.

**Expected Result:** The Guardian enters `CRITICAL` within the approved reaction
budget and reports the overtemperature-critical DTC.

**Expected Mitigations:** `DriverWarningOvertemp`.

**Evidence and Verdict Focus:** Capture the valid sample, threshold, state
transition, DTC/DFM record, mitigation event, and latency.

### TS-24 Undertemperature

**HARA trace:** No matching HARA fault ID or safety goal is currently defined;
catalog fault: Undertemperature.

**Preconditions:** Blocked until the safe charging/operating lower limit,
responsible vehicle component, and charging-control interface are specified.

**Stimulus:** Once specified, inject one valid temperature below the approved
lower limit without any other invalid input.

**Expected Result:** Apply the approved cold-temperature operating response.
The limit and Guardian/vehicle behavior are currently undefined, so no
pass/fail oracle can yet be assigned.

**Expected Mitigations:** `BlockCharging` is catalog-proposed and not an
existing Guardian mitigation.

**Evidence and Verdict Focus:** **Blocked:** record the approved limit, owner,
requirement, and interface before executing this case as a pass/fail test.

### TS-25 FreshnessLost

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

### TS-26 CounterStuck

**HARA trace:** HE-1 to HE-3; SG-2; F-1; catalog fault: CounterStuck.

**Preconditions:** Establish valid monitoring and confirm the source
`AliveCounter` is advancing normally.

**Stimulus:** Continue sending frames while holding the source `AliveCounter`
constant; do not alter the temperature fields independently.

**Expected Result:** Under FSR-2.3, the Guardian reports a source counter-stuck
fault and enters `DEGRADED` when its timeout/unchanged-counter criteria are
met.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`.

**Evidence and Verdict Focus:** Capture each frame's alive counter, publisher
sequence, source timestamp, detection window, Guardian status/DTC, and diagnostic
record.

### TS-27 SignalStuck: all temperature channels frozen

**HARA trace:** HE-1 to HE-3; SG-2; F-1; catalog fault: SignalStuck.

**Preconditions:** Establish valid monitoring with all three temperature
signals and freshness metadata observable.

**Stimulus:** Hold `CellTempMax`, `CellTempAvg`, and `CellTempMin` constant while
source timestamps and `AliveCounter` continue to advance.

**Expected Result:** Characterize whether the catalog claim is detectable.
Current FSR-2.4 detects a frozen maximum only when average or minimum moves; it
does not specify detection when all temperature values remain constant.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` only if an
implemented detector declares monitoring `DEGRADED`.

**Evidence and Verdict Focus:** Record all temperature and freshness fields
through the observation window. If no fault is reported, mark the catalog's
*(implemented)* claim as not demonstrated; do not count the limitation probe as
a passing detection test.

### TS-28 QualityInvalid

**HARA trace:** SG-2; no dedicated candidate fault ID exists in the HARA yet;
catalog fault: QualityInvalid.

**Preconditions:** Establish valid monitoring with fresh samples.

**Stimulus:** Publish one fresh sample whose quality is `INVALID` or
`ERROR_NOT_AVAILABLE`, with no other sample field or transport fault.

**Expected Result:** The Guardian rejects the sample, enters `DEGRADED`, and
reports a quality-invalid fault within `T_react`. Thermal state is not lowered.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`; no new
overtemperature mitigation from invalid quality alone.

**Evidence and Verdict Focus:** Record quality, source timestamp, alive counter,
state/status transition, DTC, DFM/OpenSOVD record, and latency. Add a dedicated
candidate malfunction ID to the HARA before claiming complete HARA traceability.

### TS-30 OutOfRange: low value

**HARA trace:** HE-1 to HE-3; SG-2/SG-4; F-6; catalog fault: OutOfRange.

**Preconditions:** Establish `WARNING` with otherwise valid input and confirm
the approved `[θ_min, θ_max]` range.

**Stimulus:** Inject one fresh, quality-valid `CellTempMin` below `θ_min`, with
the other sample fields in range.

**Expected Result:** The sample is rejected and an out-of-range fault is
reported. The active thermal state is not lowered. Monitoring follows the
approved invalid-input response.

**Expected Mitigations:** Preserve the active warning; report
`DriverWarningMonitoringUnavailable` if monitoring enters `DEGRADED`.

**Evidence and Verdict Focus:** Record range limits, injected value, state
before/after, monitoring status, fault, diagnostic record, and latency.

### TS-31 OutOfRange: high value

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-6; catalog fault: OutOfRange.

**Preconditions:** Establish `MONITORING` below `CRITICAL` with valid input and
confirm `θ_max`.

**Stimulus:** Inject one fresh sample with `CellTempMax` above `θ_max`; keep
quality, timestamp, and counters valid.

**Expected Result:** The sample is rejected and reported out of range. Thermal
state is at least `WARNING`, since real danger cannot be excluded, but invalid
input alone does not cause `CRITICAL` or mitigation.

**Expected Mitigations:** No overtemperature mitigation from this sample alone;
monitoring-unavailable warning if monitoring becomes `DEGRADED`.

**Evidence and Verdict Focus:** Record value, limit, state/status, DTC, warning,
mitigation events, and latency. Fail if the warning is suppressed or invalid
input alone triggers mitigation.

### TS-32 RateImplausible

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-8; catalog fault:
RateImplausible.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`r_max`, `Δ_res`, and timestamp configuration.

**Stimulus:** Inject one fresh sample whose maximum-temperature rise exceeds
`r_max × Δt + Δ_res`; do not inject any other invalid field.

**Expected Result:** The sample is rejected and a rate-implausible fault is
reported. The state rises to at least `WARNING` because real runaway cannot be
excluded; the invalid sample alone does not cause `CRITICAL` or mitigation.

**Expected Mitigations:** No mitigation from this sample alone;
`DriverWarningMonitoringUnavailable` if the approved response degrades
monitoring.

**Evidence and Verdict Focus:** Record old/new values, source timestamps, Δt,
computed rate, state/status, warning/DTC, mitigation, and latency. Resolve the
FSR-3.3 versus FSR-3.5 isolated-invalid status rule before final pass/fail.

### TS-33 Fast-Heating Trend

**HARA trace:** HE-1 to HE-3; SG-1; F-7; catalog fault: Fast-Heating Trend.

**Preconditions:** Establish valid, fresh monitoring below `θ_warn`; confirm
approved `r_trend` and `T_trend`.

**Stimulus:** Apply a valid rise meeting `r_trend` for `T_trend` while maximum
temperature remains below `θ_warn`.

**Expected Result:** The Guardian enters `WARNING` or a more severe state
within `T_trend + T_react`.

**Expected Mitigations:** `DriverWarning` is proposed by the catalog but is not
an existing Guardian mitigation; record the actual output.

**Evidence and Verdict Focus:** Capture the valid profile, rate, duration,
state transition, warning event, and latency. Mark blocked/failed against the
catalog claim if FSR-1.3 remains unimplemented.

### TS-36 Startup Without Source

**HARA trace:** HE-1 to HE-3; SG-2; F-9; catalog fault: Startup Without Source.

**Preconditions:** Start a fresh Guardian while the CAN source is stopped.

**Stimulus:** Publish no sample from startup through `T_stale`.

**Expected Result:** The Guardian enters `DEGRADED` and reports a startup fault
within `T_stale + T_react`; missing data is not treated as safe.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`.

**Evidence and Verdict Focus:** Record Guardian start, confirm no input sample
arrived, timeout, status/fault event, DFM record, and latency. Do not also
inject a post-start source dropout in this case.

### TS-37 Isolated Spike

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-8; catalog fault:
Isolated vs. Repeated Spike.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`r_max`, `N_suspect`, and `T_suspect`.

**Stimulus:** Inject exactly one fresh sample with a rise above `r_max`, then
resume valid nominal samples.

**Expected Result:** The sample is discarded and the isolated-invalid response
is reported. At least `WARNING` is retained because the rise may be real; no
invalid-only `CRITICAL` or mitigation is allowed.

**Expected Mitigations:** `DiscardSample` is catalog-proposed; record the
actual event. No monitoring-unavailable warning unless its configured debounce
criterion is reached.

**Evidence and Verdict Focus:** Record the single spike, surrounding valid
samples, status/state, warning/DTC, and mitigation. Resolve FSR-3.3/FSR-3.5
status behavior before declaring a pass.

### TS-38 Repeated Spike

**HARA trace:** HE-1, HE-4, and HE-5; SG-1/SG-3; F-8; catalog:
Isolated vs. Repeated Spike.

**Preconditions:** Establish valid monitoring below `CRITICAL`; record
`N_suspect` and `T_suspect`.

**Stimulus:** Inject only repeated instances of the same rate-implausible spike
within `T_suspect`, reaching `N_suspect`.

**Expected Result:** Monitoring becomes `DEGRADED` and a signal/rate fault is
reported. Thermal state is not lowered; invalid spikes alone do not cause
`CRITICAL` or mitigation.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`; no
overtemperature mitigation from invalid spikes alone.

**Evidence and Verdict Focus:** Record every spike, timestamps, debounce
window/count, status/state, DTC, DFM record, and output events.

### TS-39 Isolated Counter Error

**HARA trace:** HE-1 to HE-3; SG-2/SG-3; F-3/F-5; catalog:
Counter Error, Isolated vs. Repeated.

**Preconditions:** Establish valid input with consecutive source alive-counter
values and publisher sequence numbers.

**Stimulus:** Inject one sample whose source alive counter does not advance by
exactly one, then resume normal counter progression.

**Expected Result:** The sample is ignored and monitoring becomes `SUSPECT`;
one counter error alone does not cause `DEGRADED`.

**Expected Mitigations:** `DiscardSample` is catalog-proposed; no
`DriverWarningMonitoringUnavailable` unless the degraded threshold is reached.

**Evidence and Verdict Focus:** Capture counter values, publisher sequence,
status transition, diagnostic event, and subsequent valid recovery samples.

### TS-40 Repeated Counter Error

**HARA trace:** HE-1 to HE-3; SG-2/SG-3; F-3/F-5; catalog:
Counter Error, Isolated vs. Repeated.

**Preconditions:** Establish valid monitoring and record `N_suspect` and
`T_suspect`.

**Stimulus:** Inject only repeated alive-counter discontinuities, reaching
`N_suspect` errors within `T_suspect`.

**Expected Result:** Monitoring becomes `DEGRADED` and the Guardian reports a
counter-error fault.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable`.

**Evidence and Verdict Focus:** Record each counter value/timestamp, debounce
count/window, status transition, DTC, and DFM record.

### TS-41 Heartbeat Loss: Process Termination

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

### TS-42 Heartbeat Loss: Evaluation Hang

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

### TS-43 Stale Timestamp

**HARA trace:** HE-1 to HE-3; SG-2; F-2; catalog: Stale Timestamp.

**Preconditions:** Configure synchronized source and Guardian clocks and
establish regular valid samples.

**Stimulus:** Continue regular delivery but give each sample a source timestamp
older than `T_age`; inject no other fault.

**Expected Result:** With FSR-2.8 enabled, stale samples do not count as fresh.
When no fresh sample is accepted for `T_stale`, monitoring becomes `DEGRADED`
and a freshness fault is reported.

**Expected Mitigations:** `DriverWarningMonitoringUnavailable` after freshness
loss is declared.

**Evidence and Verdict Focus:** Record clock synchronization, source timestamp,
arrival time, computed age, sample acceptance, timeout, and fault/DFM event. If
clocks are not synchronized, mark blocked rather than infer age.

### TS-47 Upper-Scale Saturation

**HARA trace:** HE-1 to HE-3; SG-1/SG-2; catalog: Upper-Scale Saturation.

**Preconditions:** Establish valid input and confirm raw encoding maximum and
configured plausible range.

**Stimulus:** Peg one temperature channel at its encoding maximum over fresh
samples while other channels and counters remain nominal.

**Expected Result:** Apply the defined high out-of-range behavior: reject the
sample, report the fault, and raise at least `WARNING` because real danger
cannot be excluded. Do not claim a saturation-specific detection unless a
separate saturation rule exists.

**Expected Mitigations:** No invalid-only `CRITICAL`/overtemperature
mitigation; monitoring-unavailable warning if the approved range response
degrades monitoring.

**Evidence and Verdict Focus:** Record raw value, scaling, plausible limit,
quality, counter, state, DTC, and mitigation. If indistinguishable from generic
OutOfRange, record saturation-specific coverage as missing.

## AI Assistance

This document was revised with the assistance of **GitHub Copilot (GPT-6 Luna)**.

The watchdog coverage of DFR-5, TS-12, TS-13 and TS-15 was updated with the
assistance of **Claude Code** using the model **Claude Opus 5.5**
(`claude-opus-5-5`).
