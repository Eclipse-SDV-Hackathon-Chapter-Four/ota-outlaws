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
"""Portability checks for configuration shared between hosts."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("bench_controller", Path(__file__).parents[1] / "controller.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class LocalConfig(unittest.TestCase):
    def test_paths_follow_config_location(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "local.toml"
            path.write_text('base = "assets/base.qcow2"\nair = "assets/air"\nca = "assets/ca.pem"\ncleo_command = ["tools/cleo", "--flag"]\n')
            config = bench.local_config(path)
            self.assertEqual(config["base"], str((Path(directory) / "assets/base.qcow2").resolve()))
            self.assertEqual(json.loads(config["cleo_command"]), [str((Path(directory) / "tools/cleo").resolve()), "--flag"])

    def test_typo_and_credentials_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "local.toml"
            for content in ('backed = "https://example.org"', 'password = "secret"', 'cleo_command = "shell command"', 'backend = false'):
                path.write_text(content)
                with self.assertRaises(ValueError):
                    bench.local_config(path)

    def test_guest_cpu_sizes_are_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "local.toml"
            path.write_text('source_cpus = 2\ndut_cpus = 4\nsource_memory = "1G"')
            config = bench.local_config(path)
            self.assertEqual(config["source_cpus"], 2)
            for value in ("0", "9", "true", '"2"'):
                path.write_text("source_cpus = " + value)
                with self.assertRaises(ValueError):
                    bench.local_config(path)

    def test_native_cli_is_resolved_from_path(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "local.toml"
            path.write_text('cleo_command = ["opendut-cleo"]')
            self.assertEqual(json.loads(bench.local_config(path)["cleo_command"]), ["opendut-cleo"])


if __name__ == "__main__":
    unittest.main()
