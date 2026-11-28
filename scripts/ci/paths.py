#!/usr/bin/env python3
"""Gives the path filters of the workflows of each app, from the crates of the workspace.

Usage:
  paths.py

A PR runs the workflows of an app only if it changes a path in the filter of
that app. A crate of Vale that an `isotherm-` crate uses is in the filter of
both apps. The files of the workspace and of CI are also in both filters. If
the dependencies of a crate change, run this script and copy its lists into
the workflows. The test of this script fails while a workflow has an old list.
"""

import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# The changes to these paths touch both apps.
BOTH = (
    ".github/**",
    "Cargo.lock",
    "Cargo.toml",
    "assets/**",
    "scripts/ci/**",
    "scripts/ipad-run.sh",
    "scripts/testflight/**",
    "vendor/**",
)
# The paths outside the crates that touch Isotherm only.
ISOTHERM_ONLY = ("docs/isotherm/**", "scripts/isotherm-ipad.sh")
# The workflows of each app.
WORKFLOWS = {
    "vale": ("test.yml", "ios-vale.yml"),
    "isotherm": ("test-isotherm.yml", "ios-isotherm.yml"),
}
SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")


def dependencies(manifest):
    """Returns the names of all the dependencies of a crate, for each target."""
    names = set()
    for table in [manifest] + list(manifest.get("target", {}).values()):
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
    """Returns the Vale crates that an `isotherm-` crate uses, directly or through other crates."""
    found = set()
    todo = [name for name in graph if name.startswith("isotherm-")]
    while todo:
        for dep in graph.get(todo.pop(), ()):
            if dep in graph and dep not in found:
                found.add(dep)
                todo.append(dep)
    return {name for name in found if not name.startswith("isotherm-")}


def filters(graph):
    """Returns the path filter of each app."""
    shared = [f"crates/{name}/**" for name in sorted(used_by_isotherm(graph))]
    return {
        # Vale gets each path, except the paths of Isotherm only.
        "vale": ["**", "!crates/isotherm-*/**"] + [f"!{path}" for path in ISOTHERM_ONLY],
        "isotherm": ["crates/isotherm-*/**", *ISOTHERM_ONLY, *shared, *BOTH],
    }


def workflow_filter(text):
    """Returns the `paths` list of the `pull_request` event of a workflow."""
    found = re.search(r"^  pull_request:\n    paths:\n((?:      - .*\n)+)", text, re.M)
    if not found:
        return None
    return [line.strip()[2:].strip().strip("'\"") for line in found.group(1).splitlines()]


def main():
    for app, paths in filters(crates()).items():
        print(f"{app}:")
        print("\n".join(f'      - "{path}"' for path in paths))


if __name__ == "__main__":
    main()
