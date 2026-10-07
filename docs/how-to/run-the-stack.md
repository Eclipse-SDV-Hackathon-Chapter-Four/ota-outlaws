<!--
Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Eclipse Public License 2.0 which is available at
https://www.eclipse.org/legal/epl-2.0

SPDX-License-Identifier: EPL-2.0
-->

# Build and Run the Whole Stack

Builds and starts every service of [`docker-compose.yml`](../../docker-compose.yml),
including the [dashboard](../reference/components/dashboard.md), from a fresh
checkout. Commands are for PowerShell in the repository root; the ones marked
Git Bash need a POSIX shell (Git Bash on Windows, any shell on Linux and macOS).

## 1. Once: prerequisites

- Docker Desktop installed and running. If it does not start on Windows, run
  `wsl --update` and start it again.
- Git, and the repository:
  `git clone https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/ota-outlaws.git`

## 2. Once: log in to GHCR

DFM and the OpenSOVD gateway use the diagnostics image published on the
GitHub Container Registry (`diagnostics/image.env`). The package is private,
so Docker needs a GitHub token to pull it.

1. On GitHub: **Settings → Developer settings → Personal access tokens →
   Tokens (classic) → Generate new token (classic)**. Select only the scope
   **`read:packages`** and set an expiry. Use a classic token: GHCR does not
   accept fine-grained tokens for `docker login`.
2. Copy the token. GitHub shows it once. Never commit it or paste it into
   chats or issues.
3. Log in; enter the **token** as the password:

   ```powershell
   docker logout ghcr.io
   docker login ghcr.io -u YOUR-GITHUB-NAME
   ```

   `docker logout` first removes a stale login that would otherwise be used
   and rejected. Docker Desktop stores the new login.

If the pull still answers `denied`, your account has no access to the
package: an organization owner has to grant it, or make the package public.
Until then, use step 4b.

## 3. Update the code

```powershell
git checkout main
git pull
```

`unable to unlink …` means a file is open in another program, for example the
IDE: close it and pull again.

## 4a. Pull the diagnostics image (needs step 2)

```powershell
docker compose pull opensovd-dfm opensovd-gateway
```

## 4b. Or build it locally (no GHCR access)

Only needed while 4a fails and `local/opensovd-demo-fork:verified` does not
exist yet. In Git Bash:

```sh
git clone https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Doctor-Whodunit.git ../Doctor-Whodunit
sh diagnostics/build-images.sh ../Doctor-Whodunit
```

The script builds the pinned source revision, so the image matches the
published one. Before every start, in the same PowerShell session:

```powershell
$env:DIAGNOSTICS_IMAGE = "local/opensovd-demo-fork:verified"
```

## 5. Build and start everything

```powershell
docker compose up --build -d
```

The first build compiles the Rust services and takes several minutes; later
builds reuse the cache.

## 6. Check

```powershell
docker compose ps
```

Every container should be `Up`, the gateway `healthy`.

- Dashboard: <http://localhost:8080>
- OpenSOVD: `Invoke-RestMethod http://localhost:7690/sovd/v1/apps/battery_guardian/faults`
  (in PowerShell, `curl` is an alias of another command)
- Logs: `docker compose logs -f guardian`

## 7. Stop

```powershell
docker compose down
```

The stored faults survive; `docker compose down -v` deletes the DFM storage
volume too.

## Parts of the stack

| What | Command |
|------|---------|
| Only the dashboard | `docker compose up -d --no-deps dashboard` |
| Rebuild one service | `docker compose up --build -d guardian` |
| Run a campaign | Dashboard → **Campaign** tab → **Start campaign**. It uses the diagnostics image of the running stack, so start the stack first |

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `denied` when pulling the diagnostics image | Token missing, expired, or without `read:packages` (step 2), or no access to the package; use step 4b |
| DFM restarts again and again | `diagnostics/entrypoint.sh` has Windows line endings (`core.autocrlf=true`). In Git Bash: `sed -i 's/\r$//' diagnostics/entrypoint.sh`, then `docker compose up -d --force-recreate opensovd-dfm opensovd-gateway guardian watchdog` |
| `catalog hash mismatch` in the Guardian or watchdog log | The fault catalog changed while DFM kept the old one: `docker compose up -d --force-recreate opensovd-dfm opensovd-gateway guardian watchdog` |
| A port is in use | Set another one before starting: `DASHBOARD_PORT`, `SOVD_PORT`, `KUKSA_HOST_PORT`, `ZENOH_HOST_PORT`, for example `$env:DASHBOARD_PORT=8090` |
| Docker commands hang | Restart Docker Desktop |

## AI Assistance

This document was created with the assistance of **Claude Code** using the model
**Claude Opus 5.5** (`claude-opus-5-5`).
