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
# AI-assisted: GitHub Copilot / GPT-6 Luna (GPT-6 Luna)

"""Check local Markdown links and selected architecture/implementation facts."""

from __future__ import annotations

import base64
import html
import re
import sys
import tomllib
import xml.etree.ElementTree as ET
import urllib.parse
import zlib
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
LINK_PATTERN = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
HEADING_PATTERN = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$")
HTML_ANCHOR_PATTERN = re.compile(r'''(?:id|name)=["']([^"']+)["']''', re.I)


def markdown_without_fences(text: str) -> str:
    """Keep line numbering while excluding link-like examples in code fences."""
    result = []
    fence_char = None
    fence_size = 0
    for line in text.splitlines():
        marker = re.match(r"^\s*(`{3,}|~{3,})", line)
        if marker:
            fence = marker.group(1)
            if fence_char is None:
                fence_char, fence_size = fence[0], len(fence)
            elif fence[0] == fence_char and len(fence) >= fence_size:
                fence_char = None
            result.append("")
        elif fence_char is None:
            result.append(line)
        else:
            result.append("")
    return "\n".join(result)


def heading_slug(title: str) -> str:
    title = re.sub(r"`([^`]*)`", r"\1", title).lower()
    title = re.sub(r"[^\w -]", "", title)
    return re.sub(r"\s+", "-", title.strip())


def markdown_anchors(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8", errors="replace")
    anchors = {unquote(value).lower() for value in HTML_ANCHOR_PATTERN.findall(text)}
    occurrences: dict[str, int] = {}
    for line in text.splitlines():
        match = HEADING_PATTERN.match(line)
        if not match:
            continue
        slug = heading_slug(match.group(1))
        count = occurrences.get(slug, 0)
        occurrences[slug] = count + 1
        anchors.add(f"{slug}-{count}" if count else slug)
    return anchors


def check_markdown_links() -> list[str]:
    errors = []
    markdown_files = [
        path for path in ROOT.rglob("*.md")
        if not any(part in {".git", "target", "node_modules", ".venv"} for part in path.parts)
    ]
    for source in markdown_files:
        text = markdown_without_fences(source.read_text(encoding="utf-8", errors="replace"))
        for match in LINK_PATTERN.finditer(text):
            raw_target = match.group(1).strip().split(maxsplit=1)[0].strip("<>")
            parsed = urlsplit(raw_target)
            if parsed.scheme or parsed.netloc:
                continue
            target = unquote(parsed.path)
            destination = (source.parent / target).resolve() if target else source.resolve()
            line = text.count("\n", 0, match.start()) + 1
            location = f"{source.relative_to(ROOT)}:{line}"
            if not destination.exists():
                errors.append(f"{location}: missing link target {raw_target}")
                continue
            if parsed.fragment and destination.is_file() and destination.suffix.lower() == ".md":
                if unquote(parsed.fragment).lower() not in markdown_anchors(destination):
                    errors.append(f"{location}: missing heading anchor {raw_target}")
    return errors


def check_architecture_claims() -> list[str]:
    errors = []
    architecture = (ROOT / "docs/reference/architecture.md").read_text(encoding="utf-8")
    catalog = tomllib.loads((ROOT / "components/campaign/scenarios.toml").read_text(encoding="utf-8"))
    scenario = next(
        (item for item in catalog["scenario"] if item["id"] == "source_loss_during_diagnostics_outage"),
        None,
    )
    if scenario is None or scenario.get("status") != "implemented" or "TS-27" not in scenario.get("hara_tests", []):
        errors.append("components/campaign/scenarios.toml: TS-27 must map to the implemented diagnostics-outage scenario")
    if "source_loss_during_diagnostics_outage" not in architecture:
        errors.append("docs/reference/architecture.md: document the catalogued TS-27 scenario")

    compose = (ROOT / "deploy/docker-compose.yml").read_text(encoding="utf-8")
    if not re.search(r"(?m)^  watchdog:\s*$", compose):
        errors.append("deploy/docker-compose.yml: missing watchdog service")
    watchdog = (ROOT / "components/watchdog/src/main.rs").read_text(encoding="utf-8")
    if "SUPERVISOR_EVENTS" not in watchdog or "DRIVER_WARNING_MONITORING_UNAVAILABLE" not in watchdog:
        errors.append("components/watchdog/src/main.rs: expected SupervisorEvent warning publication")
    if "SupervisorEvent" not in architecture or "DRIVER_WARNING_MONITORING_UNAVAILABLE" not in architecture:
        errors.append("docs/reference/architecture.md: document the watchdog SupervisorEvent warning request")

    diagram_path = ROOT / "docs/reference/media/container-view.drawio.svg"
    diagram = ET.parse(diagram_path).getroot()
    labels = {
        " ".join(node.text.split())
        for node in diagram.iter()
        if node.tag.rsplit("}", 1)[-1] in {"text", "tspan"} and node.text and node.text.strip()
    }
    for label in {"AZ3166 Sensor ECU", "Campaign UDP Bridge", "Rust Campaign Tool", "Scenario Catalog", "DFM + OpenSOVD"}:
        if label not in labels:
            errors.append(f"{diagram_path.relative_to(ROOT)}: missing current component label {label!r}")
    for label in {"Board UDP Bridge", "Inject Faulty CAN Frames", "Inject faulty VSS Data", "Scenario Description"}:
        if label in labels:
            errors.append(f"{diagram_path.relative_to(ROOT)}: obsolete component label {label!r}")

    try:
        mxfile = ET.fromstring(html.unescape(diagram.attrib["content"]))
        embedded = mxfile.find(".//diagram")
        graph_xml = urllib.parse.unquote(
            zlib.decompress(
                base64.b64decode(urllib.parse.unquote(embedded.text)), -15
            ).decode("utf-8")
        )
        model = ET.fromstring(graph_xml)
    except (KeyError, AttributeError, TypeError, ValueError, zlib.error, ET.ParseError) as error:
        errors.append(f"{diagram_path.relative_to(ROOT)}: invalid embedded Draw.io source: {error}")
        return errors

    objects = {
        node.attrib["id"]: node
        for node in model.iter()
        if node.attrib.get("id")
        and any(child.tag.rsplit("}", 1)[-1] == "mxCell" for child in node)
    }
    embedded_names = {
        node.attrib["c4Name"]
        for node in objects.values()
        if "c4Name" in node.attrib
    }
    for name in {"AZ3166 Sensor ECU", "Campaign UDP Bridge", "Rust Campaign Tool", "Scenario Catalog", "DFM + OpenSOVD"}:
        if name not in embedded_names:
            errors.append(f"{diagram_path.relative_to(ROOT)}: embedded Draw.io source missing {name!r}")
    if {"Board UDP Bridge", "Scenario Description"} & embedded_names:
        errors.append(f"{diagram_path.relative_to(ROOT)}: embedded Draw.io source contains obsolete labels")
    bridge_edge = next(
        child for child in objects["TX55ATfUeXjnWcZuEHFn-18"]
        if child.tag.rsplit("}", 1)[-1] == "mxCell"
    )
    if (
        bridge_edge.attrib.get("source") != "TX55ATfUeXjnWcZuEHFn-17"
        or bridge_edge.attrib.get("target") != "TX55ATfUeXjnWcZuEHFn-8"
    ):
        errors.append(f"{diagram_path.relative_to(ROOT)}: campaign bridge must launch the Rust campaign tool")
    return errors


def main() -> int:
    errors = check_markdown_links() + check_architecture_claims()
    if errors:
        print("Documentation checks failed:")
        print("\n".join(f"- {error}" for error in errors))
        return 1
    print("Documentation links and architecture claims are consistent.")
    return 0


if __name__ == "__main__":
    sys.exit(main())