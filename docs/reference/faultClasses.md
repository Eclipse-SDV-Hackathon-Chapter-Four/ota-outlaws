<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Fault Classes

The fault classes of the challenge and who handles them. The Guardian detects a
fault and warns; the Evidence Collector finds out what caused it (see
[Responsibilities](architecture.md#responsibilities-guardian-and-evidence-collector)).
The requirements for each fault are in the
[traceability table](../explanation/safety-concept.md#traceability) of the Safety
Concept.

| Class | Examples | Detected by | Explained by |
|---|---|---|---|
| **Transport** | delay · duplicate · drop · reorder | Guardian: freshness timeout and alive counter | Evidence Collector: where the stream stopped, sequence numbers, delay |
| **Signal** | stuck value · spike · drift · out-of-range | Guardian: signal checks | — |
| **Source** | dropout · replay interruption | Guardian: freshness timeout and alive counter | Evidence Collector: where the stream stopped |
| **Diagnostics** | delayed DFM write · partial OpenSOVD visibility | Guardian: its own DFM writes | Evidence Collector: visibility through OpenSOVD |

## AI Assistance

This document was revised with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
