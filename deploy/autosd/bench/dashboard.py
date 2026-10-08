#!/usr/bin/env python3
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
"""Loopback dashboard adapter. The existing controller owns VMs and cleanup."""
import argparse
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import tomllib
import uuid

from dashboard_io import atomic_json

ROOT = Path(__file__).resolve().parents[3]
CONTROLLER = Path(__file__).with_name('controller.py')


def now():
    return datetime.now(timezone.utc).isoformat()


def read(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return default


def scenarios():
    catalog = tomllib.loads((ROOT / 'components/campaign/scenarios.toml').read_text())
    return [s['id'] for s in catalog['scenario']
            if s['stimulus']['type'] != 'external'
            and not s['stimulus'].get('watchdog')
            and s['stimulus'].get('isolate') in (None, 'can-link')]


def needs_cleanup(job):
    if not job:
        return False
    state = Path(job['state_dir'])
    cleanup = read(state / 'cleanup.json', {})
    owned = read(state / 'owned.json', {})
    return bool(cleanup.get('failures') or (owned and not owned.get('cleanup_complete')))


class Jobs:
    def __init__(self, config, directory=None, reports=None):
        self.config = Path(config).resolve()
        self.directory = Path(directory or ROOT / 'runs/.dashboard-opendut')
        self.reports = Path(reports or ROOT / 'runs/opendut-dashboard')
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.reports.mkdir(parents=True, exist_ok=True)
        self.lock = threading.RLock()
        self.process = None
        self.job = read(self.directory / 'latest.json', {})
        # A restarted service cannot assume a previous controller has finished.
        self.recovery = bool(self.job.get('running') or needs_cleanup(self.job))
        if self.recovery:
            self.job['phase'] = 'Service restarted; use Stop campaign to recover cleanup'

    def save(self):
        atomic_json(self.directory / 'latest.json', self.job)

    def status(self):
        with self.lock:
            status = dict(self.job)
            status.update(backend='opendut', available=self.config.is_file() and not self.recovery,
                          unavailable=None if self.config.is_file() and not self.recovery
                          else 'Configure the bench or finish interrupted cleanup',
                          exists=bool(self.job), running=bool(self.job.get('running')),
                          capabilities=['distributed-source', 'can-link-isolation'], recovery=self.recovery)
            if self.job:
                state = Path(self.job['state_dir'])
                phase = read(state / 'dashboard-phase.json', {})
                if phase and not self.recovery and status['running'] and not self.job.get('cancel_requested'):
                    status['phase'] = phase['phase']
                try:
                    lines = (state / 'dashboard.log').read_text(errors='replace').splitlines()[-300:]
                except OSError:
                    lines = []
                runtime = read(state / 'dashboard-runtime.json', {})
                lines += runtime.get('campaign_log', [])
                status['log'] = [dict(ts_ms=int(time.time()*1000), source='opendut', text=s) for s in lines]
                status['sync_error'] = read(state / 'dashboard-sync-error.json')
                status['cleanup'] = read(state / 'cleanup.json')
            return status

    def start(self, request):
        with self.lock:
            if self.job.get('running') or self.recovery:
                raise ValueError('An OpenDUT job is already active; wait for its cleanup')
            if not self.config.is_file():
                raise ValueError('Bench configuration does not exist')
            ids = request.get('scenarios', [])
            if not isinstance(ids, list) or any(not isinstance(s, str) or s not in scenarios() for s in ids):
                raise ValueError('Unknown or unsupported OpenDUT scenario')
            if len(ids) != len(set(ids)):
                raise ValueError('Duplicate scenarios')
            job = 'opendut-' + uuid.uuid4().hex[:12]
            state = self.directory / job
            state.mkdir(mode=0o700)
            # Bench application images are always built by its controller.
            command = [sys.executable, '-u', str(CONTROLLER), 'run', '--config', str(self.config),
                       '--state', str(state), '--run-id', job, '--dashboard-out', str(self.reports), *ids]
            public = self.reports / job
            atomic_json(public / 'plan.json', dict(campaign_id=job, started_at=now(), scenarios=ids or scenarios()))
            self.job = dict(run_id=job, state_dir=str(state), running=True, exists=True,
                            phase='checking prerequisites', started_at=now(), args=['run', *ids],
                            state='running', exit_code=None, finished_at=None)
            try:
                with (state / 'dashboard.log').open('w') as log:
                    self.process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=log)
            except OSError as error:
                self.process = None
                self.job.update(running=False, state='exited', exit_code=2, phase='failed', finished_at=now())
                (state / 'dashboard.log').write_text('Controller launch failed: ' + str(error) + '\n')
                self.save()
                raise
            self.job['pid'] = self.process.pid
            self.save()
            threading.Thread(target=self.watch, args=(self.process,), daemon=True).start()
            return {'done': ['Started AutoSD/OpenDUT campaign ' + job]}

    def watch(self, process):
        code = process.wait()
        with self.lock:
            cancelled = self.job.get('cancel_requested', False)
            self.job.update(running=False, state='exited', exit_code=code, finished_at=now(),
                            phase='cancelled' if cancelled else ('finished' if code == 0 else 'failed'), cancel_requested=False)
            if needs_cleanup(self.job):
                self.recovery = True
                self.job['phase'] = 'Cleanup failed; use Stop campaign to retry'
            self.save()

    def stop(self):
        with self.lock:
            if self.recovery:
                state = Path(self.job['state_dir'])
                # Refuse to race a surviving controller after a service restart.
                pid = self.job.get('pid')
                if pid:
                    result = subprocess.run(['ps', '-p', str(pid), '-o', 'command='],
                                            text=True, capture_output=True)
                    if str(CONTROLLER) in result.stdout and str(state) in result.stdout:
                        os.kill(pid, signal.SIGTERM)
                        raise ValueError('Original controller is restoring; wait for it to finish before retrying cleanup')
                command = [sys.executable, '-u', str(CONTROLLER), 'cleanup', '--config', str(self.config), '--state', str(state)]
                with (state / 'dashboard.log').open('a') as log:
                    self.process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=log)
                self.recovery = False
                self.job.update(running=True, phase='retrying cleanup', state='running', pid=self.process.pid)
                self.save()
                threading.Thread(target=self.watch, args=(self.process,), daemon=True).start()
                return {'done': ['Retrying owned OpenDUT resources cleanup']}
            if self.process and self.process.poll() is None:
                self.process.send_signal(signal.SIGTERM)
                self.job['phase'] = 'cancelling; waiting for restoration and cleanup'
                self.job['cancel_requested'] = True
                self.save()
                return {'done': ['Cancellation requested; evidence collection and cleanup are still running']}
            return {'done': ['No OpenDUT campaign is running']}

    def fault(self, code):
        if not re.fullmatch(r'[A-Za-z0-9_.-]+', code):
            raise ValueError('Invalid DTC code')
        with self.lock:
            if not self.job.get('running') or self.recovery:
                raise ValueError('No live AutoSD diagnostics; see campaign reports')
            state = Path(self.job['state_dir'])
            owned = read(state / 'owned.json', {})
            if not owned.get('campaign_unit'):
                raise ValueError('AutoSD workload is still preparing')
            peer = owned['peers'][1]
            command = ['ssh', '-p', str(peer['ssh_port']), '-i', str(state / 'key'),
                       '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=3',
                       '-o', 'StrictHostKeyChecking=yes', '-o', 'UserKnownHostsFile=' + str(state / 'known_hosts'),
                       'root@127.0.0.1',
                       'curl --max-time 3 -fs http://127.0.0.1:17690/sovd/v1/apps/battery_guardian/faults/' + code]
        result = subprocess.run(command, capture_output=True, text=True, timeout=5)
        if result.returncode:
            raise ValueError('AutoSD DTC is unavailable while the workload prepares or restores')
        return json.loads(result.stdout)

    def runtime(self):
        status = self.status()
        runtime = read(Path(self.job['state_dir']) / 'dashboard-runtime.json', {}) if self.job else {}
        # Never present a removed VM's final snapshot as a currently running stack.
        if not status['running'] or status.get('phase') != 'running' or int(time.time()*1000) - runtime.get('updated_ms', 0) > 15000:
            runtime['components'] = []
            runtime['logs'] = {}
            runtime['faults'] = None
        runtime.update(runner=status, project='AutoSD / Ankaios / OpenDUT',
                       campaign_projects=[], updated_ms=runtime.get('updated_ms', int(time.time()*1000)),
                       error=runtime.get('sovd_error'))
        return runtime


class Handler(BaseHTTPRequestHandler):
    def dispatch(self):
        # Custom header with no CORS support prevents browser drive-by mutations.
        if self.headers.get('X-Dashboard') != '1' or self.headers.get('Origin'):
            self.respond(403, {'error':'Missing dashboard header or cross-origin request'})
            return
        try:
            if self.command == 'GET' and self.path == '/status':
                result = self.server.jobs.status()
            elif self.command == 'GET' and self.path == '/runtime':
                result = self.server.jobs.runtime()
            elif self.command == 'GET' and self.path.startswith('/faults/'):
                result = self.server.jobs.fault(self.path[len('/faults/'):])
            elif self.command == 'POST' and self.path in ('/start', '/stop'):
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 <= size <= 16384:
                    raise ValueError('Request too large')
                request = json.loads(self.rfile.read(size) or b'{}')
                if not isinstance(request, dict):
                    raise ValueError('Expected a JSON object')
                result = self.server.jobs.start(request) if self.path == '/start' else self.server.jobs.stop()
            else:
                self.respond(404, {'error':'Unknown endpoint'})
                return
            self.respond(200, result)
        except (ValueError, OSError) as error:
            self.respond(409, {'error':str(error)})

    def respond(self, code, value):
        data = json.dumps(value).encode()
        self.send_response(code)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_GET = dispatch
    do_POST = dispatch

    def log_message(self, *args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--port', type=int, default=18100)
    args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', args.port), Handler)
    server.jobs = Jobs(args.config)
    def shutdown(sig, frame):
        try:
            server.jobs.stop()
        except ValueError as error:
            print(str(error), file=sys.stderr, flush=True)
        threading.Thread(target=server.shutdown, daemon=True).start()
    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)
    print(f'OpenDUT dashboard controller listening on 127.0.0.1:{args.port}', flush=True)
    server.serve_forever()
    if server.jobs.process:
        # Keep the owning process alive until the existing controller cleans up.
        server.jobs.process.wait()
    server.server_close()


if __name__ == '__main__':
    main()
