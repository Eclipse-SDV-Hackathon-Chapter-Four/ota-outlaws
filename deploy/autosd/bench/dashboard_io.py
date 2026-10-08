# Copyright (c) 2026 Contributors to the Eclipse Foundation
#
# See the NOTICE file(s) distributed with this work for additional
# information regarding copyright ownership.
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0 which is available at
# https://www.eclipse.org/legal/epl-2.0
#
# SPDX-License-Identifier: EPL-2.0
# AI-assisted: Codex / GPT-6 (gpt-6)
"""Publish only campaign evidence, never guest keys or controller credentials."""
import json
from pathlib import Path
import tarfile

FILES = {"plan.json", "campaign.md", "exit-code", "manifest.json", "report.json",
         "report.md", "recording.jsonl", "error.txt", "can-path.json"}


def atomic_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".part")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def publish(archive, destination, job):
    """Accept either the live job archive or the final campaign-reports archive."""
    destination = Path(destination) / job
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            parts = Path(member.name).parts
            if parts and parts[0] == "campaign-reports":
                parts = parts[1:]
            if not member.isfile() or not parts or parts[0] != job:
                continue
            relative = parts[1:]
            if (len(relative) not in (1, 2) or relative[-1] not in FILES
                    or any(p.startswith(".") or "/" in p or "\\" in p for p in relative)
                    or member.size > 256 * 1024 * 1024):
                continue
            target = destination.joinpath(*relative)
            target.parent.mkdir(parents=True, exist_ok=True)
            temporary = target.with_name(target.name + ".part")
            with source.extractfile(member) as data, temporary.open("wb") as output:
                while chunk := data.read(1024 * 1024):
                    output.write(chunk)
            temporary.replace(target)


# Runs on B. Read-only observation, kept outside the safety/evaluation path.
SNAPSHOT = r'''
import json, subprocess, time, urllib.request
from pathlib import Path
mapping = {'zenoh':'zenoh', 'databroker':'kuksa-databroker',
 'publisher':'vss-publisher', 'guardian':'guardian', 'dfm':'opensovd-dfm',
 'gateway':'opensovd-gateway', 'can-provider':'kuksa-can-provider'}
def command(*args):
 p=subprocess.run(args,text=True,capture_output=True,timeout=5)
 return p.stdout + p.stderr if args[:2] == ('podman','logs') else p.stdout
components=[]; logs={}
for name,service in mapping.items():
 container='ota-autosd-'+name
 try:
  data=json.loads(command('podman','inspect',container))[0]
  state=data.get('State',{}); config=data.get('Config',{})
  components.append(dict(service=service,container=container,container_id=data.get('Id'),
   state=state.get('Status','unknown'),status='Ankaios-managed on peer B',
   health=None,started_at=state.get('StartedAt'),restart_count=data.get('RestartCount',0),memory=None,
   settings=dict(image=data.get('ImageName',config.get('Image','')),env=[],ports=[],
                 command=config.get('Cmd',[]),entrypoint=config.get('Entrypoint',[]),
                 restart_policy='Ankaios',mounts=[],networks=[],ipc_mode='',pid_mode='')))
  logs[service]=command('podman','logs','--tail','60',container).splitlines()
 except (ValueError,IndexError,subprocess.TimeoutExpired):
  components.append(dict(service=service,container=None,container_id=None,state='missing',
   status='Workload preparing or restoring',health=None,started_at=None,restart_count=0,memory=None,settings=None))
faults=[]; sovd_error=None
try:
 with urllib.request.urlopen('http://127.0.0.1:17690/sovd/v1/apps/battery_guardian/faults',timeout=2) as r:
  faults=json.load(r)
except Exception as error: sovd_error=str(error)
print(json.dumps(dict(components=components,logs=logs,faults=faults,sovd_error=sovd_error,
 campaign_log=Path('/opt/ota-outlaws-autosd/bench-tools/run.log').read_text(errors='replace').splitlines()[-100:],
 updated_ms=int(time.time()*1000))))
'''
