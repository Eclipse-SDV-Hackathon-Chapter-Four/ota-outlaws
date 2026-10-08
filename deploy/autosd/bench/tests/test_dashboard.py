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
"""Controller ownership, recovery, and evidence publication regression tests."""
import io
import json
from pathlib import Path
import signal
import sys
import tarfile
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from dashboard import Jobs, scenarios
from dashboard_io import atomic_json, publish


class Process:
    # MOCK: subprocess lifecycle only; real guest campaigns are tested separately.
    pid = 12345
    def __init__(self, *args, **kwargs):
        self.event = threading.Event()
        self.code = None
    def poll(self):
        return self.code
    def wait(self):
        self.event.wait(5)
        return self.code if self.code is not None else 1
    def send_signal(self, sig):
        self.signal = sig  # Cancellation is not completion.
    def finish(self, code):
        self.code = code
        self.event.set()


class Dashboard(unittest.TestCase):
    def jobs(self, root):
        config = Path(root) / 'local.toml'
        config.write_text('backend="https://example.org"')
        return Jobs(config, Path(root) / 'private', Path(root) / 'reports')

    def test_capabilities_match_two_peer_runtime(self):
        ids = scenarios()
        self.assertIn('can_link_interruption', ids)
        self.assertIn('source_dropout_replay', ids)
        self.assertNotIn('transport_dropout', ids)
        self.assertNotIn('guardian_crash', ids)
        self.assertNotIn('guardian_hang', ids)

    def test_cancel_waits_for_cleanup_and_blocks_second_run(self):
        with tempfile.TemporaryDirectory() as root, patch('dashboard.subprocess.Popen', Process):
            jobs = self.jobs(root)
            jobs.start({'scenarios':['normal']})
            process = jobs.process
            jobs.stop()
            self.assertEqual(process.signal, signal.SIGTERM)
            self.assertTrue(jobs.status()['running'])
            with self.assertRaises(ValueError):
                jobs.start({'scenarios':['timeout']})
            process.finish(1)
            for attempt in range(100):
                if not jobs.status()['running']: break
                time.sleep(.005)
            self.assertFalse(jobs.status()['running'])
            self.assertTrue((jobs.reports / jobs.job['run_id'] / 'plan.json').is_file())

    def test_failed_cleanup_requires_recovery_and_disables_stale_runtime(self):
        with tempfile.TemporaryDirectory() as root, patch('dashboard.subprocess.Popen', Process):
            jobs = self.jobs(root)
            jobs.start({'scenarios':['normal']})
            atomic_json(Path(jobs.job['state_dir']) / 'cleanup.json', {'failures':['backend offline']})
            jobs.process.finish(1)
            for attempt in range(100):
                if not jobs.status()['running']: break
                time.sleep(.005)
            self.assertFalse(jobs.status()['available'])
            self.assertEqual(jobs.runtime()['components'], [])
            with self.assertRaises(ValueError): jobs.start({'scenarios':['normal']})
            restarted = self.jobs(root)
            self.assertTrue(restarted.status()['recovery'])
            with self.assertRaises(ValueError): restarted.start({'scenarios':['normal']})

    def test_missing_cleanup_record_blocks_restart_with_owned_resources(self):
        with tempfile.TemporaryDirectory() as root:
            jobs = self.jobs(root)
            jobs.job = {'running':False, 'state_dir':root}
            atomic_json(Path(root) / 'owned.json', {'vms':[{'pid':12345}]})
            jobs.save()
            self.assertTrue(self.jobs(root).status()['recovery'])

    def test_restart_never_assumes_previous_cleanup_complete(self):
        with tempfile.TemporaryDirectory() as root:
            jobs = self.jobs(root)
            jobs.job = {'running':True, 'state_dir':root, 'pid':12345}
            jobs.save()
            restarted = self.jobs(root)
            self.assertFalse(restarted.status()['available'])
            self.assertTrue(restarted.status()['running'])

    def test_unsupported_or_duplicate_scenarios_never_spawn(self):
        with tempfile.TemporaryDirectory() as root, patch('dashboard.subprocess.Popen') as spawn:
            jobs = self.jobs(root)
            for ids in [['guardian_hang'], ['--config'], ['normal','normal'], 'normal']:
                with self.assertRaises(ValueError): jobs.start({'scenarios':ids})
            spawn.assert_not_called()

    def test_controller_launch_failure_retains_evidence_without_stuck_job(self):
        with tempfile.TemporaryDirectory() as root:
            jobs = self.jobs(root)
            with patch('dashboard.subprocess.Popen', side_effect=OSError('process limit')):
                with self.assertRaises(OSError): jobs.start({'scenarios':['normal']})
            self.assertFalse(jobs.status()['running'])
            self.assertTrue((jobs.reports / jobs.job['run_id'] / 'plan.json').is_file())
            self.assertIn('process limit', jobs.status()['log'][-1]['text'])
            self.assertFalse(self.jobs(root).status()['recovery'])

    def test_publish_retains_reports_and_excludes_keys_and_traversal(self):
        with tempfile.TemporaryDirectory() as root:
            archive = Path(root) / 'evidence.tar.gz'
            with tarfile.open(archive, 'w:gz') as target:
                for name in ['campaign-reports/job/normal/report.json',
                             'campaign-reports/job/normal/recording.jsonl',
                             'campaign-reports/job/normal/source-key',
                             'campaign-reports/job/../error.txt',
                             'campaign-reports/other/normal/report.json']:
                    data=b'{}'; member=tarfile.TarInfo(name); member.size=len(data)
                    target.addfile(member,io.BytesIO(data))
                member=tarfile.TarInfo('campaign-reports/job/evil/report.json')
                member.type=tarfile.SYMTYPE; member.linkname='/etc/passwd'
                target.addfile(member)
            publish(archive, Path(root) / 'public', 'job')
            self.assertEqual(sorted(str(p.relative_to(Path(root)/'public')) for p in (Path(root)/'public').rglob('*') if p.is_file()),
                             ['job/normal/recording.jsonl','job/normal/report.json'])


if __name__ == '__main__':
    unittest.main()
