<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Agent Instructions – ota-outlaws

These instructions apply to every AI coding agent working in this repository
(Claude Code, Codex, Cursor, GitHub Copilot, Gemini CLI, and others).

## Project context

Team **ota-outlaws** is competing at the **Eclipse SDV Hackathon 2026**
(Friedrichshafen) on the challenge **"Doctor Whodunit"**: a portable *Safety
Evidence Factory* around a Battery Thermal Guardian, built from KUKSA, uProtocol,
Ankaios, OpenSOVD, and AutoSD.

**Before you plan or write any code, read both of these files:**

- [docs/reference/challenge.md](docs/reference/challenge.md): the full challenge,
  including architecture rules, Definition of Done, and "What Not to Do". This is
  the spec. It is copied verbatim from the organizers, so do not edit it.
- [docs/reference/rules.md](docs/reference/rules.md): hackathon rules, deadlines,
  and scoring criteria.

## Non-negotiable challenge rules

- The Guardian receives VSS data **only through a uProtocol service interface**.
  It must never read the KUKSA Data Broker directly or depend on CAN decoding
  internals.
- Fault campaigns must be **deterministic and replayable**. Do not hard-code
  machine-specific paths, hosts, or credentials; use configuration.
- Every verdict (PASS / FAIL / INCONCLUSIVE) must be backed by an evidence chain:
  hazard → safety goal → injected fault → detection → mitigation → verdict, linked
  by correlation IDs.
- Never hide or delete failed scenarios.

## Hackathon rules that affect how you work

- Only **demonstrated or repository-verifiable** work is scored. Prefer working,
  runnable code over plans and prose.
- **Never present mocked or stubbed functionality as real.** Mark it clearly in the
  code (for example `// MOCK:` or `# MOCK:`) and in the README. Judges explicitly ask
  which parts are complete, mocked, or prepared.
- All code must be written during the hackathon. Do not copy in code the team
  prepared earlier unless the user confirms it was declared to the HackMC.
- **Code freeze is Day 3 at 08:00.** After that, do not push to the submission
  branch unless the user explicitly asks you to.
- Scoring rewards reproducibility, tests, CI, documentation, clear interfaces, and
  meaningful use of Eclipse SDV projects. Bonus points go to openDUT, ThreadX,
  AutoSD, and Java when they are used in code, configuration, or deployment. A
  logo on a slide does not count.

## Required: Eclipse license header

Every new source, script, configuration, and documentation file you create must
start with the Eclipse Public License 2.0 header below, written in the file's comment
syntax. Use the current year.

```text
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
```

| File type | Comment style |
|-----------|---------------|
| Rust, C, C++, Java, JS/TS, Protobuf | `/* … */` block, or `//` on every line |
| Python, Shell, YAML, TOML, Dockerfile, Containerfile, Makefile | `#` on every line |
| Markdown, HTML, XML | `<!-- … -->` |

- Keep a shebang (`#!/usr/bin/env …`) on line 1 and put the header right after it.
- Formats without comments (JSON, generated files, binary data, CAN `.asc`
  traces) are exempt. Do not break the format to add a header.
- Do not add our header to third-party or vendored files, or to content copied
  verbatim (such as `docs/reference/challenge.md`). Keep their original license
  notices.
- When you edit an existing file that has no header, add one, unless the file is
  exempt as described above.

## Required: AI assistance disclosure

Transparency about AI use is required. State **your actual tool and model**, for
example "Claude Code / Claude Opus 5.5 (`claude-opus-5-5`)" or "GitHub Copilot /
GPT-…". Never guess or copy another agent's identity. If you do not know your exact
model, ask the user.

1. **Source and config files** that you create or substantially change get one line
   directly below the license header, in the same comment style:

   ```text
   AI-assisted: <tool> / <model name> (<model id>)
   ```

   If the line already names a different tool or model, add yours to it, separated
   by `;`. Do not replace the existing entry.

2. **Markdown documentation** that you create or substantially change ends with this
   section:

   ```markdown
   ## AI Assistance

   This document was created with the assistance of **<tool>** using the model
   **<model name>** (`<model id>`).
   ```

   If the section already exists and names a different tool or model, add yours to
   it.

3. **Commits** that you create include the trailer your tool provides (for example
   `Co-Authored-By: …`). If your tool provides none, add
   `Assisted-by: <tool> / <model name>`.

"Substantially" means more than trivial fixes such as typos, formatting, or renames.

---

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
