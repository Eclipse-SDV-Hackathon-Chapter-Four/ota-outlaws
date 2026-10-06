<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Eclipse SDV Hackathon 2026 – Rules & Evaluation

Summary of the official *Eclipse SDV Hackathon Guide Book (2026 edition)* and the
*Eclipse SDV Hackathon 2026 Evaluation Forms*. The originals are published at
<https://github.com/Eclipse-SDV-Hackathon-Chapter-Four>. If this summary and the
originals disagree, the originals win.

## 1. Ground rules

- **All code must be created during the hackathon.** Anything prepared beforehand
  (code, environment, ideas) must be declared clearly to the HackMC in person on
  Day 1. Business ideas may be prepared, but all assets must be created during
  hackathon hours.
- **Only demonstrated or repository-verifiable work counts.** Slides alone do not
  count for technical scoring.
- **Be explicit about what is complete, mocked, or prepared.** Coaches ask about
  this directly ("Code Does What It Claims").
- Teams have at most 5 people; the event is capped at 50 attendees.
- Everyone must follow the
  [Eclipse Community Code of Conduct](https://www.eclipse.org/org/documents/Community_Code_of_Conduct.php).
  Report issues to <codeofconduct@eclipse.org>.

## 2. Tracks

Two independent competitions, ranked separately. Teams choose their track on Day 1
after the challenge presentations; it cannot be changed afterwards.

| | Innovation Track (Hackathon) | Freestyle Track (HackFest) |
|---|---|---|
| Audience | Students, young developers, newcomers | Professionals, core contributors, project experts |
| Focus | Innovative edge applications backed by a **business case** | Project integration, new project features, interfacing multiple projects |
| Deliverables | Business pitch deck + (working) application demo | **Real** Eclipse project PRs or filed Eclipse SDV Blueprint issues |
| Success metrics | Pitch, solution capability, code maturity, technical expertise | PR quality, code maturity, issue resolution count, technical expertise |

## 3. Timeline & deadlines

| When | What |
|------|------|
| Day 1, 11:30–12:30 | Team & challenge selection |
| **Day 1, 18:00** | **Solution Plan** submitted to the Hack Coaches |
| Day 2 | Hacking; interview slots are assigned |
| **Day 3, 08:00** | **Code freeze** – final project and repository uploads |
| Day 3, 09:00–10:30 | Technical interview with Hack Coaches (8 minutes, live demo) |
| **Day 3, 11:00** | **Pitch deck upload deadline** |
| Day 3, 12:00–13:30 | Finalist pitches (7 finalists across both locations; 10 min pitch + 5 min Q&A) |
| Day 3, 14:30 | Award ceremony |

### Solution Plan contents (Day 1)

1. **Team at a Glance**: name/tagline, roster with GitHub handles, roles, chosen
   challenge, core idea.
2. **How Do You Work?**: lightweight development process and tracking, quality
   control (tests, docs, reviews), team communication, decision making.

## 4. Scoring

Scores run from 0 to 5 per criterion. **HackCoaches decide 80 %** (technical
substance, impact, verifiable evidence) and the **Jury decides 20 %** (pitch, story,
contribution).

| Score | Meaning |
|:-:|---|
| 0 | Not available / not shown |
| 1 | Very weak / only claimed |
| 2 | Partially fulfilled |
| 3 | Solidly fulfilled |
| 4 | Strongly fulfilled |
| 5 | Excellent / clearly above expectations |

### Innovation Track weights

| Criterion | Weight | What earns a 5 |
|---|:-:|---|
| Problem Solving | 13 % | Clear measurable value, realistic usage, convincing outcome |
| Eclipse SDV Ecosystem Integrability | **27 %** | Reusable PR, issue, Blueprint extension or proposal; multiple projects in one coherent flow |
| Development Methods | **20 %** | Mature process, architecture, QA, traceability, open-source handover |
| Working Demo | 10 % | Stable, reproducible demo with variants or failure handling |
| Code Does What It Claims | 10 % | Reproducible, understandable, well-defined CI pipelines, ready for open-source follow-up |
| Pitch & Handover Clarity (Jury) | 8 % | |
| Community Benefit & Continuation Story (Jury) | 7 % | |
| Contribution Focus & Initiative (Jury) | 5 % | |

The Innovation jury scorecard also covers pitch deck, business perspective, pitch
delivery, and creativity & surprise.

### Freestyle Track weights

| Criterion | Weight | What earns a 5 |
|---|:-:|---|
| Contribution Value | 25 % | Valuable enough that the community should continue it after the event |
| Technical Quality & Maturity | 25 % | Well-structured, documented, validated, close to merge |
| Eclipse SDV Ecosystem Impact | 20 % | Reusable Blueprint pattern, integration path, or contributor workflow |
| Reusability & Maintainability | 10 % | Easy to reproduce, extend, review, and maintain |
| Pitch & Handover Clarity (Jury) | 8 % | Jury immediately understands what happened and why it matters |
| Community Benefit & Continuation Story (Jury) | 7 % | Clear links to PRs, issues, docs, maintainers, follow-up actions |
| Contribution Focus & Initiative (Jury) | 5 % | Existing project work substantially advanced and easy to adopt |

Freestyle work on **existing** issues, PRs, gaps, or Blueprint tasks is preferred.
New ideas score highly only when they are tied to a real project need and documented
as a follow-up issue or proposal.

### Extra Technology Points (both tracks)

+0.10 each, up to +0.40 in total; final score = `MIN(5.00, base + bonus)`. A
technology only counts if it is **meaningfully used** in code, configuration,
deployment, integration, testing, a demo, a PR, an issue, or a reproducible setup. A
logo on a slide does not count.

| Technology | Counts when used as |
|---|---|
| Eclipse openDUT | Solution, test setup, integration flow, or contribution |
| Java / Jakarta EE | Implementation, API, tooling, backend, test, or integration component |
| Eclipse ThreadX | Runtime, embedded target, integration component, or demo device |
| Eclipse AutoSD | Platform, deployment base, integration target, or contribution environment |

The Doctor Whodunit challenge maps naturally onto openDUT (remote reruns), ThreadX
(AZ3166 source), and AutoSD (runtime target).

## 5. Help channels

- Slack `#ask-a-hackcoach`: technical problems
- Slack `#hackathon2026`: venue and general questions

---

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
