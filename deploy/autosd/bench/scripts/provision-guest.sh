#!/usr/bin/bash
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

# Run as root INSIDE an AutoSD guest; bench.py uploads this script and assets.
# Argument: the peer hostname. Configure backend reachability/trust, install the
# CAN tunnel tools and unpack EDGAR. bench.py separately supplies EDGAR's setup
# string and invokes its managed setup to register NetBird and create the service.
set -euo pipefail
[[ $# == 2 ]] || { echo 'Usage: provision-guest.sh HOSTNAME BACKEND_HOSTS' >&2; exit 2; }
hostnamectl set-hostname "$1"
# Wait for a trustworthy clock before certificate validity checks.
chronyc waitsync 20 0.1 0 2
# QEMU user-network gateway reaches the Mac's published Docker ports.
sed -i '/# can-testbench-host/d' /etc/hosts
printf '%s # can-testbench-host\n' "$2" >> /etc/hosts
install -m 0644 /tmp/can-testbench-provision/opendut-ca.pem /etc/pki/ca-trust/source/anchors/can-testbench-opendut-ca.pem
update-ca-trust
curl --fail --silent --show-error --connect-timeout 5 --max-time 10 ${OPENDUT_BACKEND_URL:?}/ >/dev/null
# Extract a release archive, dropping its enclosing directory. Python avoids
# installing tar in the minimal guest; the data filter rejects unsafe paths.
unpack() {
    python3 - "$1" "$2" <<'UNPACK'
import sys, tarfile
with tarfile.open(sys.argv[1], 'r:gz') as archive:
    for member in archive.getmembers():
        parts = member.name.split('/', 1)
        if len(parts) != 2 or not parts[1]:
            continue
        member.name = parts[1]
        archive.extract(member, sys.argv[2], filter='data')
UNPACK
}
mkdir -p /opt/can-testbench/cannelloni /opt/can-testbench/edgar /usr/local/lib64
unpack /tmp/can-testbench-provision/cannelloni-1.1.0-arm64.tar.gz /opt/can-testbench/cannelloni
# Avoid overwriting unchanged libraries used by a running tunnel on reruns.
if ! cmp -s /tmp/can-testbench-provision/cannelloni-tcp-fixed /usr/local/bin/cannelloni.real || [[ ! -f /usr/local/lib64/libcannelloni-common.so.0 ]]; then
    install -m 0755 /tmp/can-testbench-provision/cannelloni-tcp-fixed /usr/local/bin/cannelloni.real
    cp -a /opt/can-testbench/cannelloni/libcannelloni-common.so.0 /opt/can-testbench/cannelloni/libsctp.so* /usr/local/lib64/
    printf '/usr/local/lib64\n' > /etc/ld.so.conf.d/cannelloni.conf
    ldconfig
fi
install -m 0755 /tmp/can-testbench-provision/cannelloni-tcp /usr/local/bin/cannelloni
unpack /tmp/can-testbench-provision/edgar-0.10.2-arm64.tar.gz /opt/can-testbench/edgar
modprobe vcan
modprobe can_gw max_hops=2
systemctl restart opendut-vcan.service
cannelloni -h >/dev/null
/opt/can-testbench/edgar/opendut-edgar --version
# Preserve CAN startup ordering even when EDGAR regenerates its main unit.
mkdir -p /etc/systemd/system/opendut-edgar.service.d
cat > /etc/systemd/system/opendut-edgar.service.d/can-testbench.conf <<'UNIT'
[Unit]
# Wants orders startup without stopping EDGAR when this oneshot is restarted.
Wants=opendut-vcan.service
After=opendut-vcan.service network-online.target chronyd.service
UNIT
systemctl daemon-reload
