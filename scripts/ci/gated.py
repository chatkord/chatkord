#!/usr/bin/env python3
"""Passes when a commit's tree is one CI's gate passed, or differs from one only by files the gate does not build.

From the commit down main's first parents: a commit whose own checks passed is gated; one whose changes over its
first parent are all exempt (ci.yml's `paths-ignore`, release-please's own files) is gated if its parent is; a
merge whose tree is its second parent's is gated if that parent's checks passed. Anything else is refused with one
sentence. The checks are the contexts `.github/rulesets/main.json` requires; nothing is run, only read.

Usage: gated.py <commit>   (GH_TOKEN and GITHUB_REPOSITORY from the environment, as in a workflow)
"""
import json
import os
import re
import subprocess
import sys

root = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
# How far down main it looks for a gated tree before it gives up.
DEPTH = 50


def run(*args):
    return subprocess.run(args, cwd=root, check=True, capture_output=True, encoding="utf-8").stdout


def git(*args):
    return run("git", *args).strip()


def glob(pattern):
    """A GitHub Actions path filter as a regex: `**` crosses folders, `*` and `?` do not."""
    parts = re.split(r"(\*\*|\*|\?)", pattern)
    wild = {"**": ".*", "*": "[^/]*", "?": "[^/]"}
    return re.compile("".join(wild.get(part, re.escape(part)) for part in parts))


def exempt():
    """The paths no gate builds: what ci.yml skips, and the files a release pull request writes."""
    ci = json.loads(run("yq", "-o", "json", ".github/workflows/ci.yml"))
    patterns = [glob(p) for p in ci["on"]["pull_request"].get("paths-ignore", [])]
    config = json.load(open(os.path.join(root, "release-please-config.json"), encoding="utf-8"))
    files = {".release-please-manifest.json"}
    for package in config["packages"].values():
        files.add(package.get("changelog-path", "CHANGELOG.md"))
        files.update(package.get("extra-files", []))
        # The `simple` release type keeps its version in version.txt.
        if package.get("release-type", config.get("release-type")) == "simple":
            files.add("version.txt")
    return lambda path: path in files or any(p.fullmatch(path) for p in patterns)


def required():
    rules = json.load(open(os.path.join(root, ".github/rulesets/main.json"), encoding="utf-8"))["rules"]
    return {
        check["context"]
        for rule in rules
        if rule["type"] == "required_status_checks"
        for check in rule["parameters"]["required_status_checks"]
    }


def passed(commit, contexts, repo):
    """Every required check concluded `success` on this commit, as run by GitHub Actions."""
    runs = json.loads(
        run("gh", "api", "--paginate", "--slurp", f"repos/{repo}/commits/{commit}/check-runs?filter=latest")
    )
    won = {
        check["name"]
        for page in runs
        for check in page["check_runs"]
        if check["app"]["slug"] == "github-actions" and check["conclusion"] == "success"
    }
    return contexts <= won


def parents(commit):
    return git("rev-list", "--parents", "-n", "1", commit).split()[1:]


def gated(commit, repo):
    """None when the commit's tree was gated, else the sentence that says why not."""
    skips = exempt()
    contexts = required()
    for _ in range(DEPTH):
        short = git("rev-parse", "--short", commit)
        if passed(commit, contexts, repo):
            return None
        above = parents(commit)
        if not above:
            return f"{short} is the first commit and no gate passed on it."
        changed = git("diff", "--name-only", above[0], commit).splitlines()
        if all(skips(path) for path in changed):
            commit = above[0]
            continue
        if len(above) == 2 and git("rev-parse", f"{commit}^{{tree}}") == git("rev-parse", f"{above[1]}^{{tree}}"):
            if passed(above[1], contexts, repo):
                return None
            return f"{short} merges {git('rev-parse', '--short', above[1])}, on which the gate did not pass."
        return f"{short} changes what the gate builds, and no gate passed on that tree."
    return f"No gated tree within {DEPTH} commits below {git('rev-parse', '--short', commit)}."


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    commit = git("rev-parse", "--verify", f"{sys.argv[1]}^{{commit}}")
    refusal = gated(commit, os.environ["GITHUB_REPOSITORY"])
    if refusal:
        print(f"::error::{refusal}")
        sys.exit(1)
    print(f"{commit} was gated.")


if __name__ == "__main__":
    main()
