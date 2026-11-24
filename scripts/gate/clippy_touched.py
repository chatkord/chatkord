#!/usr/bin/env python3
"""The packages a commit's clippy lints, one workspace a line: those whose files are staged, changed or untracked, and
every package of the workspaces CI's rule reaches (scripts/ci/changes.py) that depends on one through a path, however
far. A path in a workspace's directory that no member holds (its lock, its cargo config), or in none (the gate, the
protocol submodule, the toolchain), lints each workspace it reaches whole. Packages are read from `cargo metadata --no-deps`.

Usage: clippy_touched.py          lines of `WORKSPACE PACKAGE...`, or `WORKSPACE --workspace`
       clippy_touched.py --self-test
"""
import json
import os
import subprocess
import sys

sys.dont_write_bytecode = True
root = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path.insert(0, os.path.join(root, "scripts", "ci"))
from changes import WORKSPACES, changed, reached  # noqa: E402  (CI's own rule of what a change reaches)

WHOLE = "--workspace"


def members(workspace):
    """Each member of `workspace` as (name, its directory, the directories of its path dependencies), relative."""
    found = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"],
        cwd=os.path.join(root, workspace), check=True, capture_output=True, encoding="utf-8").stdout
    relative = lambda path: os.path.relpath(path, root).replace(os.sep, "/")  # noqa: E731
    return [
        (package["name"], relative(os.path.dirname(package["manifest_path"])),
         [relative(dep["path"]) for dep in package["dependencies"] if dep.get("path")])
        for package in json.loads(found)["packages"]
    ]


def holds(directory, path):
    return path == directory or path.startswith(directory + "/")


def plan(paths, workspaces, members_of):
    """{workspace: sorted packages, or [WHOLE]} for `paths`, over the `workspaces` they reach."""
    packages = {ws: members_of(ws) for ws in workspaces}
    every = [(ws, name, directory, deps) for ws, found in packages.items() for name, directory, deps in found]
    whole, touched = set(), set()
    for path in paths:
        if path.endswith(".md"):
            continue
        holding = [entry for entry in every if holds(entry[2], path)]
        if holding:
            touched.add(max(holding, key=lambda entry: len(entry[2]))[2])
        else:
            mine = [ws for ws in workspaces if holds(ws, path)]
            whole.update(mine or reached([path]))
    # Every package that reaches a touched one through its path dependencies.
    grew = True
    while grew:
        grew = False
        for _, _, directory, deps in every:
            if directory not in touched and any(dep in touched for dep in deps):
                touched.add(directory)
                grew = True
    result = {}
    for ws, found in packages.items():
        if ws in whole:
            result[ws] = [WHOLE]
        elif names := sorted(name for name, directory, _ in found if directory in touched):
            result[ws] = names
    return {ws: result[ws] for ws in WORKSPACES if ws in result}


def self_test():
    fixture = {
        # A dependent listed before what it depends on, so one pass over them is not enough.
        "client": [
            ("shell", "client/shell", ["client/screens"]),
            ("screens", "client/screens", ["client/terms"]),
            ("terms", "client/terms", []),
            ("words", "client/words", []),
        ],
        "fuzz": [("fuzz", "fuzz", ["client/terms"])],
    }
    cases = {
        "a package and what depends on it, however far": (
            (["client/terms/src/lib.rs"], ["client", "fuzz"]), {"client": ["screens", "shell", "terms"], "fuzz": ["fuzz"]}),
        "a package nothing depends on lints alone": (
            (["client/words/src/lib.rs"], ["client"]), {"client": ["words"]}),
        "a workspace's own file outside its members lints it whole": (
            (["client/Cargo.lock"], ["client"]), {"client": [WHOLE]}),
        "a path no workspace holds lints each it reaches whole": (
            (["protocol/proto/src/lib.rs"], ["client", "fuzz"]), {"client": [WHOLE], "fuzz": [WHOLE]}),
        "prose lints nothing": ((["client/terms/README.md", "README.md"], ["client"]), {}),
    }
    wrong = [name for name, ((paths, workspaces), want) in cases.items()
             if plan(paths, workspaces, fixture.get) != want]
    for name in wrong:
        print(f"clippy_touched self-test: wrong answer for {name}")
    if not wrong:
        print("clippy_touched self-test: every shape is answered")
    return 1 if wrong else 0


def main(args):
    sys.stdout.reconfigure(newline="\n")
    if args == ["--self-test"]:
        return self_test()
    if args:
        print(__doc__)
        return 2
    paths = changed("HEAD")
    for ws, names in plan(paths, reached(paths), members).items():
        print(ws, *names)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
