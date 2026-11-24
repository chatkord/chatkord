#!/usr/bin/env python3
"""Fails when a git pin's retiring release is published, a native source a recipe pins by tag has a later release,
or one it pins by branch (a project that publishes no tags) has moved past the pinned commit; when the engine crates
(webrtc-sys, libwebrtc) or the engine's branch have a later release; when a base image's tag names another digest than
the Containerfile's; when the newest stable Rust is not rust-toolchain.toml's; or when mise has a later release than the
Containerfile's.
Prints one line per finding and nothing else when none is. A locked crate behind is not a finding:
.github/workflows/update.yml moves the locks once a month.

A pin's retiring release is stated once, beside the pin: `[workspace.metadata.retired-by]` in a workspace's
Cargo.toml (crate = version), and `FFMPEG_UNTIL` in scripts/deps/ffmpeg-pin.sh. A recipe's tag is its `<NAME>_TAG=`
line and the `fetch` that takes it, a branch's its `<NAME>_BRANCH=` and `<NAME>_COMMIT=` line and the `fetch_branch`
that takes it (scripts/deps/source.sh). Run weekly by .github/workflows/audit.yml.

Usage: released.py | --self-test
"""
import json
import os
import re
import subprocess
import sys
import tomllib
import urllib.request

root = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
INDEX = "https://index.crates.io"
FFMPEG = "https://github.com/FFmpeg/FFmpeg.git"
ENGINE_REPOSITORY = "https://github.com/webrtc-sdk/webrtc.git"
RUST_STABLE = "https://static.rust-lang.org/dist/channel-rust-stable.toml"
MISE_LATEST = "https://api.github.com/repos/jdx/mise/releases/latest"
MANIFESTS = ", ".join([
    "application/vnd.oci.image.index.v1+json",
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
])


def run(args, cwd=root):
    return subprocess.run(args, cwd=cwd, check=True, capture_output=True, encoding="utf-8")


def version(text):
    """A release's numbers, or None for a pre-release (which never retires a pin)."""
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", text)
    return tuple(int(part) for part in match.groups()) if match else None


def index_path(name):
    """Where crates.io's sparse index keeps a crate (its documented layout)."""
    name = name.lower()
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def published(name):
    with urllib.request.urlopen(f"{INDEX}/{index_path(name)}", timeout=30) as answer:
        lines = answer.read().decode("utf-8").splitlines()
    entries = [json.loads(line) for line in lines if line]
    return [entry["vers"] for entry in entries if not entry.get("yanked")]


def retired_crates(workspace):
    meta = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"], os.path.join(root, workspace)).stdout)
    return ((meta.get("metadata") or {}).get("retired-by") or {}).items()


def workspaces():
    """Every workspace: each tracked Cargo.lock's directory."""
    locks = run(["git", "ls-files", "--", "*Cargo.lock"]).stdout.split()
    return [os.path.dirname(lock) for lock in locks]


def ffmpeg_until():
    with open(os.path.join(root, "scripts", "deps", "ffmpeg-pin.sh"), encoding="utf-8") as pin:
        found = re.search(r"^FFMPEG_UNTIL=(\S+)$", pin.read(), re.MULTILINE)
    if not found:
        sys.exit("FAIL: scripts/deps/ffmpeg-pin.sh names no FFMPEG_UNTIL")
    return found.group(1)


def recipes():
    """Each script under scripts/deps, by name: its text."""
    deps = os.path.join(root, "scripts", "deps")
    texts = {}
    for recipe in sorted(f for f in os.listdir(deps) if f.endswith(".sh")):
        with open(os.path.join(deps, recipe), encoding="utf-8") as script:
            texts[recipe] = script.read()
    return texts


def pinned(pattern, fetched):
    """Each pin `pattern` finds in any script that a `fetched` line of any script takes, once: (file, name, url, *pin).
    A pin may live in a file the recipes source, so every script's pins are read first."""
    texts = recipes()
    pins = {var: (recipe, *pin) for recipe, text in texts.items()
            for var, *pin in re.findall(pattern, text, re.MULTILINE)}
    seen = set()
    for text in texts.values():
        for name, url, var in re.findall(fetched, text, re.MULTILINE):
            if var not in seen:
                seen.add(var)
                recipe, *pin = pins[var]
                yield recipe, name, url, *pin


def tag_pins():
    """Each source a recipe fetches by tag: (file, name, url, tag)."""
    return pinned(r"^(\w+)_TAG=(\S+?);", r'^fetch (\S+) (\S+) "\$(\w+)_TAG"')


def branch_pins():
    """Each source a recipe fetches by branch: (file, name, url, branch, commit)."""
    return pinned(r"^(\w+)_BRANCH=(\S+?);\s+\1_COMMIT=([0-9a-f]{40})$", r'^fetch_branch (\S+) (\S+) "\$(\w+)_BRANCH"')


def release(tag):
    """A release tag's prefix and numbers (`mesa-26.2.6` is `mesa-`, (26, 2, 6)); None for a candidate or other tag."""
    found = re.fullmatch(r"(\D*?)(\d+(?:\.\d+)*)", tag)
    return (found.group(1), tuple(int(n) for n in found.group(2).split("."))) if found else None


def successor(pinned, other):
    """Whether `other` is a later release of the same line: its prefix and length, the same major or the next.
    An old scheme's tags (libva's dates, libvpl's years) are neither."""
    (prefix, numbers), (its_prefix, its_numbers) = pinned, other
    return (its_prefix == prefix and len(its_numbers) == len(numbers) and its_numbers > numbers
            and its_numbers[0] - numbers[0] in (0, 1))


def get(url, headers=None):
    """An answer's lower-cased headers and its text."""
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers or {}), timeout=30) as answer:
        return {name.lower(): value for name, value in answer.headers.items()}, answer.read().decode("utf-8")


def read(path):
    with open(os.path.join(root, path), encoding="utf-8") as file:
        return file.read()


def engine_pins(recipe):
    """The crates the engine recipe pins (`declare -A VERSION=([name]=x.y.z ...)`) and its branch (`ENGINE_BRANCH=`)."""
    table = re.search(r"^declare -A VERSION=\(([^)]*)\)", recipe, re.MULTILINE)
    branch = re.search(r"^ENGINE_BRANCH=(\S+)$", recipe, re.MULTILINE)
    if not table or not branch:
        sys.exit("FAIL: scripts/deps/webrtc.sh names no VERSION table or no ENGINE_BRANCH")
    return dict(re.findall(r"\[([\w-]+)\]=(\S+)", table.group(1))), branch.group(1)


def later_crates(pins, listing):
    """A finding for each pinned crate with a later stable release; `listing` gives a crate's published versions."""
    found = []
    for name, wanted in pins.items():
        newest = max((v for v in listing(name) if version(v)), key=version, default=None)
        if newest and version(newest) > version(wanted):
            found.append(f"scripts/deps/webrtc.sh: {name} {newest} is published (pinned {wanted})")
    return found


def later_branch(branch, heads):
    """A finding when the engine's repository has a release branch of a later milestone (`m150_release` -> `m151_release`)."""
    milestone = lambda name: int(m.group(1)) if (m := re.fullmatch(r"m(\d+)_release", name)) else None
    pinned = milestone(branch)
    later = sorted(n for n in (milestone(h.removeprefix("refs/heads/")) for h in heads) if n and pinned and n > pinned)
    return [f"scripts/deps/webrtc.sh: the engine's branch m{later[-1]}_release exists (pinned {branch})"] if later else []


def base_images(dockerfile):
    """Each `FROM <image>:<tag>@sha256:<digest>` of the Containerfile: (image with its tag, digest)."""
    return re.findall(r"^FROM (\S+?)@sha256:([0-9a-f]{64})", dockerfile, re.MULTILINE)


def registry_digest(image, fetch):
    """The digest a Docker Hub tag names now, asked anonymously: a pull token, then the manifest's own header."""
    name, _, tag = image.rpartition(":")
    host, _, path = name.partition("/")
    if host != "docker.io":
        sys.exit(f"FAIL: scripts/ci/builder.Containerfile: {image} is not on Docker Hub, the only registry released.py asks")
    _, token = fetch(f"https://auth.docker.io/token?service=registry.docker.io&scope=repository:{path}:pull")
    headers, _ = fetch(f"https://registry-1.docker.io/v2/{path}/manifests/{tag}",
                       {"Authorization": f"Bearer {json.loads(token)['token']}", "Accept": MANIFESTS})
    return headers["docker-content-digest"].removeprefix("sha256:")


def moved_images(dockerfile, fetch):
    """A finding for each base image whose tag names another digest than the one pinned."""
    return [f"scripts/ci/builder.Containerfile: {image} names another digest now: re-pin it"
            for image, digest in base_images(dockerfile) if registry_digest(image, fetch) != digest]


def stable_rust(channel):
    """The version of the stable channel's manifest (`[pkg.rust] version = "1.100.0 (hash date)"`)."""
    return tomllib.loads(channel)["pkg"]["rust"]["version"].split()[0]


def newer_toolchain(toolchain, channel):
    """A finding when the stable Rust is later than rust-toolchain.toml's."""
    pinned = tomllib.loads(toolchain)["toolchain"]["channel"]
    newest = stable_rust(channel)
    if version(pinned) is None:
        sys.exit(f"FAIL: rust-toolchain.toml: {pinned!r} is not a version")
    return [f"rust-toolchain.toml: Rust {newest} is stable (pinned {pinned})"] if version(newest) > version(pinned) else []


def newer_mise(dockerfile, latest):
    """A finding when mise's latest release (a GitHub release answer) is later than the Containerfile's `MISE_VERSION`."""
    pinned = re.search(r"^ARG MISE_VERSION=(\S+)$", dockerfile, re.MULTILINE)
    if not pinned:
        sys.exit("FAIL: scripts/ci/builder.Containerfile names no MISE_VERSION")
    newest = json.loads(latest)["tag_name"].removeprefix("v")
    # mise numbers its releases by year and month: 2026.10.25 is a (year, month, patch) like any other.
    return [f"scripts/ci/builder.Containerfile: mise {newest} is released (pinned {pinned.group(1)})"] if (
        version(newest) and version(newest) > version(pinned.group(1))) else []


def self_test():
    sha = "a" * 64
    other = "b" * 64
    pins = {"rust": "sha256:" + sha, "debian": "sha256:" + other}
    dockerfile = (f"FROM docker.io/library/rust:1.100.0-trixie@{pins['rust']} AS base\n"
                  f"ARG MISE_VERSION=2026.10.25\n"
                  f"FROM docker.io/library/debian:trixie-slim@{pins['debian']} AS runtime\nFROM runtime\n")
    answers = {}

    def fetch(url, headers=None):
        if url.startswith("https://auth.docker.io/token"):
            return {}, json.dumps({"token": "t"})
        assert headers["Authorization"] == "Bearer t", "the pull token is sent"
        return {"docker-content-digest": "sha256:" + answers[url.rsplit("/", 3)[-3] + "/" + url.rsplit("/", 1)[-1]]}, ""

    answers.update({"rust/1.100.0-trixie": sha, "debian/trixie-slim": other})
    recipe = "declare -A VERSION=([webrtc-sys]=0.3.54 [libwebrtc]=0.4.1)\nENGINE_BRANCH=m150_release\n"
    pins, branch = engine_pins(recipe)
    channel = '[pkg.rust]\nversion = "1.101.0 (abcdef0 2026-11-01)"\n'
    toolchain = '[toolchain]\nchannel = "1.100.0"\n'
    cases = {
        "the engine crates are read": (pins, {"webrtc-sys": "0.3.54", "libwebrtc": "0.4.1"}),
        "a later crate is found, a yanked-looking pre-release is not": (
            later_crates(pins, lambda name: ["0.3.54", "0.3.55-rc1"] if name == "webrtc-sys" else ["0.4.2"]),
            ["scripts/deps/webrtc.sh: libwebrtc 0.4.2 is published (pinned 0.4.1)"]),
        "the same crates are no finding": (later_crates(pins, lambda name: [pins[name]]), []),
        "a later engine branch is found": (
            later_branch(branch, ["refs/heads/m149_release", "refs/heads/m150_release", "refs/heads/m151_release", "refs/heads/main"]),
            ["scripts/deps/webrtc.sh: the engine's branch m151_release exists (pinned m150_release)"]),
        "the same engine branch is no finding": (later_branch(branch, ["refs/heads/m150_release", "refs/heads/dev"]), []),
        "both images are read": (base_images(dockerfile), [("docker.io/library/rust:1.100.0-trixie", sha),
                                                          ("docker.io/library/debian:trixie-slim", other)]),
        "an image at its digest is no finding": (moved_images(dockerfile, fetch), []),
        "a moved tag is found": (
            moved_images(dockerfile, lambda url, headers=None: ({"docker-content-digest": "sha256:" + other}, "")
                         if "manifests" in url else fetch(url, headers)),
            ["scripts/ci/builder.Containerfile: docker.io/library/rust:1.100.0-trixie names another digest now: re-pin it"]),
        "a later stable Rust is found": (newer_toolchain(toolchain, channel), ["rust-toolchain.toml: Rust 1.101.0 is stable (pinned 1.100.0)"]),
        "the stable Rust itself is no finding": (newer_toolchain(toolchain, channel.replace("1.101.0", "1.100.0")), []),
        "a later mise is found": (newer_mise(dockerfile, '{"tag_name": "v2026.11.2"}'),
                                  ["scripts/ci/builder.Containerfile: mise 2026.11.2 is released (pinned 2026.10.25)"]),
        "the pinned mise is no finding": (newer_mise(dockerfile, '{"tag_name": "v2026.10.25"}'), []),
        "a later release of a line is a successor, a candidate is not": (
            (successor(release("mesa-26.2.6"), release("mesa-26.3.0")), release("v1.0.0-rc1")), (True, None)),
    }
    wrong = [name for name, (got, want) in cases.items() if got != want]
    for name in wrong:
        print(f"released self-test: wrong answer for {name}")
    if not wrong:
        print("released self-test: every watch is answered")
    return 1 if wrong else 0


def main():
    findings = []
    for workspace in workspaces():
        for name, wanted in retired_crates(workspace):
            target = version(wanted)
            if target is None:
                sys.exit(f"FAIL: {workspace}/Cargo.toml: {name}'s retiring release {wanted!r} is not a version")
            out = [vers for vers in published(name) if (version(vers) or (0, 0, 0)) >= target]
            if out:
                findings.append(f"{workspace}: {name} {max(out, key=version)} is published: retire its git pin")
    until = ffmpeg_until()
    tags = run(["git", "ls-remote", "--tags", FFMPEG, f"refs/tags/{until}*"]).stdout.split()
    # The release and its point releases; `n9.1-dev` marks where master began the cycle and is no release.
    if any(re.fullmatch(rf"refs/tags/{re.escape(until)}(\.\d+)*", tag) for tag in tags):
        findings.append(f"FFmpeg {until} is tagged: retire the master pin in scripts/deps/ffmpeg-pin.sh")
    for recipe, name, url, tag in tag_pins():
        pinned = release(tag)
        listed = run(["git", "ls-remote", "--tags", url]).stdout.split()
        later = [t for t in (ref.removeprefix("refs/tags/") for ref in listed[1::2]) if (r := release(t)) and pinned
                 and successor(pinned, r)]
        if later:
            findings.append(f"scripts/deps/{recipe}: {name} {max(later, key=lambda t: release(t)[1])} is released")
    for recipe, name, url, branch, commit in branch_pins():
        head = run(["git", "ls-remote", url, f"refs/heads/{branch}"]).stdout.split()[:1]
        if head != [commit]:
            findings.append(f"scripts/deps/{recipe}: {name}'s {branch} has moved past the pinned commit")
    recipe = read("scripts/deps/webrtc.sh")
    pins, branch = engine_pins(recipe)
    findings += later_crates(pins, published)
    heads = run(["git", "ls-remote", "--heads", ENGINE_REPOSITORY, "refs/heads/m*_release"]).stdout.split()[1::2]
    findings += later_branch(branch, heads)
    findings += moved_images(read("scripts/ci/builder.Containerfile"), get)
    findings += newer_toolchain(read("rust-toolchain.toml"), get(RUST_STABLE)[1])
    findings += newer_mise(read("scripts/ci/builder.Containerfile"), get(MISE_LATEST)[1])
    for finding in findings:
        print(finding)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(self_test() if sys.argv[1:] == ["--self-test"] else main())
