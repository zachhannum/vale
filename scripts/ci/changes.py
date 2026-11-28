#!/usr/bin/env python3
"""Tells which apps a change touches, so CI builds only those apps.

Usage:
  changes.py [BASE HEAD]

The script reads the changed files between the two commits and writes
`vale=true` or `vale=false`, `isotherm=true` or `isotherm=false`, and the list
of the apps as JSON, for example `apps=["vale"]`, to the file that
GITHUB_OUTPUT names, and to the standard output. With no commits, it writes
that the change touches both apps.

A crate of Isotherm touches Isotherm. A crate of Vale touches Vale, and it also
touches Isotherm if an `isotherm-` crate depends on it. The files of the
workspace and of CI touch both apps.
"""

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APPS = ("vale", "isotherm")
# The changes to these paths touch both apps.
BOTH = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "vendor/",
    "assets/",
    ".github/",
    "scripts/ci/",
    "scripts/testflight/",
    "scripts/ipad-run.sh",
)
# The changes to these paths outside `crates/` touch Isotherm only.
ISOTHERM = ("docs/isotherm/", "scripts/isotherm-ipad.sh")
SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")


def dependencies(manifest):
    """Returns the names of all the dependencies of a crate, for each target."""
    names = set()
    tables = [manifest] + list(manifest.get("target", {}).values())
    for table in tables:
        for section in SECTIONS:
            names.update(table.get(section, {}))
    return names


def crates(root=ROOT):
    """Returns the dependencies of each crate of the workspace, by the name of its folder."""
    out = {}
    for path in sorted((root / "crates").glob("*/Cargo.toml")):
        with open(path, "rb") as file:
            out[path.parent.name] = dependencies(tomllib.load(file))
    return out


def used_by_isotherm(graph):
    """Returns the crates that an `isotherm-` crate uses, directly or through other crates."""
    found = set()
    todo = [name for name in graph if name.startswith("isotherm-")]
    while todo:
        name = todo.pop()
        for dep in graph.get(name, ()):
            if dep in graph and dep not in found:
                found.add(dep)
                todo.append(dep)
    return found


def apps_of(path, graph, shared):
    """Returns the apps that a change to one file touches."""
    if path.startswith(BOTH):
        return set(APPS)
    if path.startswith(ISOTHERM):
        return {"isotherm"}
    parts = path.split("/")
    if parts[0] == "crates" and len(parts) > 2:
        name = parts[1]
        if name.startswith("isotherm-"):
            return {"isotherm"}
        return {"vale", "isotherm"} if name in shared else {"vale"}
    return {"vale"}


def touched(paths, graph):
    shared = used_by_isotherm(graph)
    apps = set()
    for path in paths:
        apps |= apps_of(path, graph, shared)
    return apps


def output(apps):
    """Returns the lines of GITHUB_OUTPUT for the apps that a change touches."""
    lines = [f"{app}={'true' if app in apps else 'false'}" for app in APPS]
    return lines + ["apps=" + json.dumps([app for app in APPS if app in apps])]


def main():
    if len(sys.argv) == 3 and all(sys.argv[1:]):
        base, head = sys.argv[1:]
        names = subprocess.run(
            ["git", "diff", "--name-only", f"{base}...{head}"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout.split()
        apps = touched(names, crates())
    elif len(sys.argv) == 1 or (len(sys.argv) == 3 and not any(sys.argv[1:])):
        apps = set(APPS)
    else:
        sys.exit(__doc__)
    lines = output(apps)
    print("\n".join(lines))
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a") as file:
            file.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
