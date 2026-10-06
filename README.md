<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# OTA Outlaws

Project plan: [Project Plan](docs/reference/project-plan.md)

Build a local diagnostics runtime from the Doctor-Whodunit example sources.
The gateway includes the example's DFM adapter.

## Build from clean committed source

```sh
sh diagnostics/build-images.sh /path/to/Doctor-Whodunit
```

The source revision is pinned to `97dd4a503f25674e866a89829e2bd92d2cf2655d` in
`build-images.sh`. This local Doctor-Whodunit revision contains separate commits for restoring omitted upstream CLI
support crates and repairing the diagnostic demo. It retains the
adapter's original Git dependency: `bburda42dot/fault-lib` at
`2b638d84a38568a70d5acab4b46cbe17a84e8e7c`. The DFM daemon still builds from the
bundled fault library. It is not unmodified upstream `opensovd-core`.

The build refuses a dirty source checkout. It uses `git archive` to include only
tracked files at the pinned commit, builds with Cargo's `--locked` option, and
makes no source patches during the build. Base images are pinned by digest. It
copies compiled binaries into a runtime image rather than saving a running
container. The runtime includes DFM, the gateway, and example Guardian and injector binaries.

The image records its source commit in `org.opencontainers.image.revision` and a
Git blob manifest at `/usr/share/opensovd/source-files.txt`. `SOURCE_REF` can select
another committed integration revision. The Dockerfile and build helper are
tracked here, so there are no undocumented build steps.

To transfer the built image:

```sh
docker save local/opensovd-demo-fork:verified -o /tmp/diagnostics-image.tar
# Transfer the archive to the other machine, then:
docker load -i /tmp/diagnostics-image.tar
```

## AI Assistance

This document was created with the assistance of **Codex** using the model
**GPT-6.1 Sol** (`gpt-6.1-sol`).
