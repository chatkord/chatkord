#!/usr/bin/env python3
"""The next release's version, by release-please's own rules over its config, its manifest and the Conventional
Commits since the last release tag reachable from HEAD (from the first commit when there is none). A tag with a
pre-release suffix (`v0.1.0-dev.<UTC>`, a dev release's) is never a release. With no release yet it is the package's
`initial-version`. Otherwise, from the manifest's version: before 1.0 with
`bump-minor-pre-major`, a breaking change bumps the minor, a `feat` the minor (the patch with
`bump-patch-for-minor-pre-major`), and anything else the patch; from 1.0 a breaking change bumps the major, a `feat`
the minor, anything else the patch. With nothing releasable since the tag it is still the patch's: no release can be
lower, so a pre-release of it never outranks the release that follows.

Usage: next.py                 the next release's version
       next.py --dev [--at T]  its pre-release for a build made at T (UTC yyyymmddHHMMSS, now when left out)
       next.py --build [--at T] the version a build of HEAD carries: the release's at its tag, else the newest dev
                                release's at its tag, else --dev's
       next.py --self-test
"""
import datetime
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "protocol", "scripts"))
from check_commit_msg import kind  # noqa: E402  (the commit-msg hook's own parser)

root = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
STAMP = "%Y%m%d%H%M%S"
RELEASE = re.compile(r"(\d+)\.(\d+)\.(\d+)")
DEV = re.compile(r"\d+\.\d+\.\d+-dev\.\d{14}")


def git(*args):
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, encoding="utf-8").stdout.strip()


def settings():
    """The root package's settings over the config's own, and the manifest's version, as release-please reads them."""
    with open(os.path.join(root, "release-please-config.json"), encoding="utf-8") as file:
        config = json.load(file)
    with open(os.path.join(root, ".release-please-manifest.json"), encoding="utf-8") as file:
        manifest = json.load(file)
    return {**config, **config["packages"]["."]}, manifest["."]


def prefix(config):
    return "v" if config.get("include-v-in-tag", True) else ""


def bump(config, version, messages):
    """The release after `version` that `messages` make."""
    major, minor, patch = (int(part) for part in version.split("."))
    kinds = [found for found in map(kind, messages) if found]
    breaking = any(breaks for _, breaks in kinds)
    feature = any(type_ == "feat" for type_, _ in kinds)
    if breaking and major == 0 and config.get("bump-minor-pre-major"):
        return f"0.{minor + 1}.0"
    if breaking:
        return f"{major + 1}.0.0"
    if feature and not (major == 0 and config.get("bump-patch-for-minor-pre-major")):
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def next_version(config, manifest, released, messages):
    """The next release: `initial-version` while none was made, else the bump `messages` make."""
    if not released:
        return config.get("initial-version", "1.0.0")
    return bump(config, manifest, messages)


def dev(version, at):
    return f"{version}-dev.{at.strftime(STAMP)}"


def latest_release(tags, mark):
    """The release tag of the highest version among `tags`, None when none is a release."""
    found = [(tuple(map(int, m.groups())), tag) for tag in tags
             if tag.startswith(mark) and (m := RELEASE.fullmatch(tag.removeprefix(mark)))]
    return max(found, default=(None, None))[1]


def built(tags, mark):
    """The version a build at a commit carrying `tags` is: its release's, else its newest dev release's, else None."""
    versions = [tag.removeprefix(mark) for tag in tags if tag.startswith(mark)]
    releases = [v for v in versions if RELEASE.fullmatch(v)]
    # By version, then by the stamp, which is fixed-width: the newest sorts last.
    order = lambda v: (tuple(map(int, RELEASE.match(v).groups())), v)  # noqa: E731
    devs = sorted((v for v in versions if DEV.fullmatch(v)), key=order)
    return (releases or devs[-1:] or [None])[-1]


def history(config):
    """Whether a release tag is reachable from HEAD, and every commit message since it (or since the first commit)."""
    tag = latest_release(git("tag", "--merged", "HEAD", "--list", f"{prefix(config)}[0-9]*").split(), prefix(config))
    span = f"{tag}..HEAD" if tag else "HEAD"
    messages = git("log", "--format=%B%x00", span).split("\0")
    return tag is not None, [m.strip() for m in messages if m.strip()]


def self_test():
    config, _ = settings()
    simple = {"include-v-in-tag": True, "bump-minor-pre-major": True, "initial-version": "0.1.0"}
    at = datetime.datetime(2026, 11, 24, 9, 30, 0, tzinfo=datetime.timezone.utc)
    cases = {
        "the repository's own config makes 0.1.0 first": (next_version(config, "0.0.0", False, ["fix: x"]), "0.1.0"),
        "before a release, a break too is the initial version": (
            next_version(simple, "0.0.0", False, ["feat!: x"]), "0.1.0"),
        "after 0.1.0 a fix is the patch": (next_version(simple, "0.1.0", True, ["fix(a): x"]), "0.1.1"),
        "after 0.1.0 a feat is the minor": (next_version(simple, "0.1.0", True, ["fix: x", "feat: y"]), "0.2.0"),
        "after 0.1.0 a break is the minor": (next_version(simple, "0.1.0", True, ["refactor!: x"]), "0.2.0"),
        "a breaking footer breaks": (
            next_version(simple, "0.1.0", True, ["chore: x\n\nBREAKING CHANGE: y"]), "0.2.0"),
        "nothing releasable is still the patch": (next_version(simple, "0.1.0", True, ["docs: x", "Merge x"]), "0.1.1"),
        "a feat with bump-patch-for-minor-pre-major is the patch": (
            next_version({**simple, "bump-patch-for-minor-pre-major": True}, "0.1.0", True, ["feat: x"]), "0.1.1"),
        "from 1.0 a break is the major": (next_version(simple, "1.2.3", True, ["fix!: x"]), "2.0.0"),
        "from 1.0 a feat is the minor": (next_version(simple, "1.2.3", True, ["feat: x"]), "1.3.0"),
        "a dev build is the next release's pre-release": (dev("0.1.0", at), "0.1.0-dev.20261124093000"),
        "a dev release's tag is no release": (latest_release(["v0.1.0-dev.20261124093000"], "v"), None),
        "the last release is the highest, dev tags aside": (
            latest_release(["v0.1.0", "v0.2.0-dev.20261124093000", "v0.1.1", "v0.10.0-rc.1"], "v"), "v0.1.1"),
        "a build at a release's tag is the release": (
            built(["v0.1.0-dev.20261124093000", "v0.1.0"], "v"), "0.1.0"),
        "a build at dev tags alone is the newest dev release": (
            built(["v0.1.0-dev.20261124093000", "v0.1.0-dev.20261125093000"], "v"), "0.1.0-dev.20261125093000"),
        "a build at no tag, or another suffix's, has none": (built(["v0.1.0-rc.1", "other"], "v"), None),
    }
    wrong = [name for name, (got, want) in cases.items() if got != want]
    for name in wrong:
        print(f"next self-test: wrong answer for {name}: {cases[name][0]}")
    if not wrong:
        print("next self-test: every shape is answered")
    return 1 if wrong else 0


def main(args):
    sys.stdout.reconfigure(newline="\n")
    if args == ["--self-test"]:
        return self_test()
    mode, at = None, datetime.datetime.now(datetime.timezone.utc)
    match args:
        case []:
            pass
        case [("--dev" | "--build") as mode]:
            pass
        case [("--dev" | "--build") as mode, "--at", stamp]:
            try:
                at = datetime.datetime.strptime(stamp, STAMP)
            except ValueError:
                print(f"next: --at is a UTC time as yyyymmddHHMMSS, not {stamp!r}")
                return 2
        case _:
            print(__doc__)
            return 2
    config, manifest = settings()
    if mode == "--build":
        tagged = built(git("tag", "--points-at", "HEAD", "--list", f"{prefix(config)}[0-9]*").split(), prefix(config))
        if tagged:
            print(tagged)
            return 0
    version = next_version(config, manifest, *history(config))
    print(dev(version, at) if mode else version)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
