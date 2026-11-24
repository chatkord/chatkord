#!/usr/bin/env python3
"""Writes notices.json, the third-party notices a package carries, into its licences folder:
one group per carried project, read from the texts already in that folder or in the build tree, then one per licence
of the Rust packages in the shipped binaries, as cargo-about finds them. The licences cargo-about accepts and the
targets it reads are deny.toml's; a pin's commit is its recipe's (`<NAME>_COMMIT=`, as released.py reads them).

Usage: notices.py <folder> <version> <package manifest>...
       notices.py --self-test   (each shape of licence text the reflow meets)
"""
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib

root = os.path.realpath(os.path.join(os.path.dirname(__file__), "..", ".."))
RECIPE = "The recipe is in this package."


def fail(said):
    sys.exit(f"FAIL: {said}")


def recipes():
    """Every recipe's text, one string: each pin is named once across them."""
    found = []
    for path in sorted(glob.glob(os.path.join(root, "scripts/deps/*.sh"))):
        with open(path, encoding="utf-8") as recipe:
            found.append(recipe.read())
    return "\n".join(found)


def pin(name, texts=None):
    """A recipe's `<name>=` value: a commit, a tag, a branch."""
    found = re.search(rf"(?:^|;\s+){name}=([^\s;]+)", texts or recipes(), re.MULTILINE)
    if not found:
        fail(f"scripts/deps names no {name}")
    return found.group(1)


def short(name):
    return pin(name)[:8]


def listed(names):
    """`a`, `a and b`, `a, b and c`."""
    return names[0] if len(names) == 1 else f"{', '.join(names[:-1])} and {names[-1]}"


def reflow(text):
    """A paragraph's hard-wrapped lines as one, so the window wraps it to its own width. A blank line ends a
    paragraph; a Markdown heading stands alone and a fence, which holds no text, goes; a bullet or a number starts an
    item, whose indented lines continue it; any other indented line keeps its break, as an indented block does."""
    lines, open_as = [], None
    for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n"):
        line = line.rstrip()
        if not line:
            if lines and lines[-1]:
                lines.append("")
            open_as = None
        elif re.match(r"(```|~~~)", line):
            open_as = None
        elif re.match(r"#+\s", line):
            lines.append(line)
            open_as = None
        elif re.match(r"\s*([-*•>]|\d+[.)]|\(\w{1,3}\))\s", line):
            lines.append(line.strip())
            open_as = "item"
        elif line[0].isspace() and open_as != "item":
            lines.append(line.strip())
            open_as = "block"
        elif open_as in ("paragraph", "item"):
            lines[-1] += " " + line.strip()
        else:
            lines.append(line)
            open_as = "paragraph"
    return "\n".join(lines).strip()


def read(folder, pattern):
    """Every file `pattern` matches under `folder`, in order; none is a package without its notices."""
    found = sorted(path for path in glob.glob(os.path.join(folder, pattern), recursive=True) if os.path.isfile(path))
    if not found:
        fail(f"{os.path.join(folder, pattern)} matches no licence text")
    return found


def texts(folder, *patterns):
    """The files' texts, each under its own path when there are several."""
    paths = [path for pattern in patterns for path in read(folder, pattern)]
    parts = []
    for path in paths:
        with open(path, encoding="utf-8") as text:
            body = reflow(text.read())
        parts.append(body if len(paths) == 1 else f"{os.path.relpath(path, folder)}\n\n{body}")
    return "\n\n\n".join(parts)


def group(name, what, licence, text, source=None):
    entry = {"name": name, "what": what, "licence": licence}
    if source:
        entry["source"] = source
    entry["text"] = text
    return entry


def seclists(text):
    """The common passwords the protocol's data carries, which a new password is checked against."""
    return group("SecLists", "The common passwords a new one is checked against", "MIT", text)


def engine():
    with open(os.path.join(root, "scripts/deps/webrtc.sh"), encoding="utf-8") as recipe:
        webrtc = recipe.read()
    commit, branch = pin("ENGINE_SOURCE", webrtc), pin("ENGINE_BRANCH", webrtc)
    release = branch.removesuffix("_release")
    return f"LiveKit’s build webrtc-{commit[:7]} of webrtc-sdk {release} at commit {commit[:8]}."


def client_groups(folder, windows):
    """The client's carried projects, from the texts notices.sh put in `folder`."""
    groups = [
        group("FFmpeg", "Decodes and encodes video", "GPL-2.0-or-later", texts(folder, "ffmpeg/*"),
              f"Built from commit {short('FFMPEG_COMMIT')} with our patches. {RECIPE}"),
        group("x264", "Encodes H.264 when no graphics card can", "GPL-2.0-or-later", texts(folder, "x264/*"),
              f"Built from commit {short('X264_COMMIT')}. {RECIPE}"),
        group("WebRTC", "Calls: sound, pictures and the network", "BSD-3-Clause and others",
              texts(folder, "libwebrtc/LICENSE.md"), engine()),
        group("dav1d and SVT-AV1", "AV1 pictures", "BSD-2-Clause, BSD-3-Clause-Clear",
              texts(folder, "dav1d/*", "svt-av1/*"),
              f"Built from commits {short('DAV1D_COMMIT')} and {short('SVT_AV1_COMMIT')}. SVT-AV1 comes with the "
              "Alliance for Open Media’s patent licence."),
    ]
    if windows:
        groups.append(group(
            "AMF, libvpl and nv-codec-headers", "Reach AMD, Intel and NVIDIA graphics cards’ video engines", "MIT",
            texts(folder, "amf/*", "libvpl/*", "nv-codec-headers/*"),
            f"Built from commits {listed([short('AMF_COMMIT'), short('LIBVPL_COMMIT'), short('NVCODEC_COMMIT')])}."))
    else:
        groups += [
            group("libva and libdrm", "Reach the graphics card’s video engine", "MIT",
                  texts(folder, "libva/*", "libdrm/*"),
                  f"Built from commits {short('LIBVA_COMMIT')} and {short('LIBDRM_COMMIT')}. {RECIPE}"),
            group("Mesa", "The AMD graphics cards’ video driver", "MIT and others", texts(folder, "mesa/**/*"),
                  f"Built from commit {short('MESA_COMMIT')}. {RECIPE}"),
        ]
    return groups + [
        group("Manrope and IBM Plex Mono", "The fonts", "OFL-1.1", texts(folder, "Manrope.txt", "IBMPlexMono.txt")),
        group("Twemoji", "The emoji", "CC-BY-4.0", texts(folder, "Twemoji.txt")),
        group("Lucide", "The icons", "ISC", texts(folder, "Lucide.txt")),
        seclists(texts(folder, "SecLists/*")),
    ]


def config(target, locked):
    """cargo-about's configuration: deny.toml's licences, each exception for its crate alone where `locked` has it, our
    own crates left out."""
    with open(os.path.join(root, "deny.toml"), "rb") as deny:
        licences = tomllib.load(deny)["licenses"]
    lines = [f"accepted = {json.dumps(licences['allow'])}", f"targets = {json.dumps([target])}",
             "ignore-build-dependencies = true", "ignore-dev-dependencies = true", "private = { ignore = true }"]
    for exception in licences.get("exceptions", []):
        if exception["crate"] in locked:
            lines += ["", f"[{json.dumps(exception['crate'])}]", f"accepted = {json.dumps(exception['allow'])}"]
    return "\n".join(lines) + "\n"


def locked(manifest):
    """The names of the packages in the lock of `manifest`'s workspace."""
    folder = os.path.dirname(manifest)
    while not os.path.isfile(os.path.join(folder, "Cargo.lock")):
        if os.path.dirname(folder) == folder:
            fail(f"{manifest} has no Cargo.lock above it")
        folder = os.path.dirname(folder)
    with open(os.path.join(folder, "Cargo.lock"), "rb") as lock:
        return {package["name"] for package in tomllib.load(lock)["package"]}


def target():
    host = re.search(r"^host: (\S+)$", subprocess.run(["rustc", "-vV"], check=True, stdout=subprocess.PIPE,
                                                       encoding="utf-8").stdout, re.MULTILINE)
    if not host:
        fail("rustc names no host")
    with open(os.path.join(root, "deny.toml"), "rb") as deny:
        allowed = tomllib.load(deny)["graph"]["targets"]
    if host.group(1) not in allowed:
        fail(f"{host.group(1)} is not one of deny.toml's targets")
    return host.group(1)


def crate_groups(manifests, triple):
    """One group per licence of the Rust packages the binaries of `manifests` link, each text with the packages it
    came with."""
    by_id = {}
    with tempfile.TemporaryDirectory() as scratch:
        settings = os.path.join(scratch, "about.toml")
        for manifest in manifests:
            with open(settings, "w", encoding="utf-8") as out:
                out.write(config(triple, locked(manifest)))
            said = subprocess.run(["cargo", "about", "generate", "--format", "json", "--frozen", "--fail",
                                   "--config", settings, "--manifest-path", manifest], check=True,
                                  stdout=subprocess.PIPE, encoding="utf-8").stdout
            for licence in json.loads(said)["licenses"]:
                users = {(use["crate"]["name"], use["crate"]["version"]) for use in licence["used_by"]}
                by_id.setdefault(licence["id"], {}).setdefault(licence["text"], set()).update(users)
    groups = []
    for spdx, texts_of in by_id.items():
        crates = sorted({name for users in texts_of.values() for name, _ in users})
        named = crates[:3] + (["more"] if len(crates) > 3 else [])
        noun = "package" if len(crates) == 1 else "packages"
        parts = [f"{', '.join(f'{n} {v}' for n, v in sorted(users))}\n\n{reflow(text)}"
                 for text, users in sorted(texts_of.items(), key=lambda item: sorted(item[1]))]
        groups.append((len(crates), group(spdx, f"{len(crates)} Rust {noun}: {listed(named)}", spdx,
                                          "\n\n\n".join(parts))))
    return [entry for _, entry in sorted(groups, key=lambda pair: (-pair[0], pair[1]["name"]))]


def built(version):
    """The package itself, as About Chatkord states it: its version, and its release's tag or else the commit built."""
    def git(*args):
        return subprocess.run(["git", "-C", root, *args], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                              encoding="utf-8").stdout.strip()
    tag = f"v{version}"
    if git("describe", "--exact-match", "--tags", "HEAD") == tag:
        return {"version": version, "from": {"tag": tag}}
    commit = git("rev-parse", "--short=8", "HEAD")
    if not commit:
        fail(f"{root} is not a git checkout, so the commit built is unknown")
    return {"version": version, "from": {"commit": commit}}


def self_test():
    """Each shape of licence text the reflow meets, and what the window is given for it."""
    cases = {
        "a hard-wrapped paragraph is one line":
            ("Permission is hereby\ngranted, free of charge.\n", "Permission is hereby granted, free of charge."),
        "a blank line ends a paragraph": ("One.\n\nTwo\nthree.", "One.\n\nTwo three."),
        "a heading stands alone and a fence goes":
            ("# webrtc\n```\nCopyright 2011.\n```", "# webrtc\nCopyright 2011."),
        "an item's indented lines continue it":
            ("  * Keep the above\n    notice.\n  * Do not endorse.", "* Keep the above notice.\n* Do not endorse."),
        "a numbered item starts a line": ("Terms:\n1. Use it.\n2) Share it.", "Terms:\n1. Use it.\n2) Share it."),
        "an indented block keeps its lines":
            ("Add this:\n\n    <name>\n    Copyright <year>", "Add this:\n\n<name>\nCopyright <year>"),
        "Windows line ends read as one": ("A\r\nB\r\n", "A B"),
    }
    wrong = [name for name, (text, want) in cases.items() if reflow(text) != want]
    for name in wrong:
        print(f"notices self-test: wrong reflow for {name}")
    if not wrong:
        print("notices self-test: every shape is reflowed")
    return 1 if wrong else 0


def main(argv):
    if argv[1:] == ["--self-test"]:
        sys.exit(self_test())
    if len(argv) < 4:
        fail("usage: notices.py <folder> <version> <package manifest>...")
    folder, version = argv[1], argv[2]
    manifests = [os.path.abspath(manifest) for manifest in argv[3:]]
    triple = target()
    # What follows reads every locked package's manifest, another system's too, which a build never fetched.
    for manifest in manifests:
        subprocess.run(["cargo", "fetch", "--locked", "--manifest-path", manifest], check=True)
    natives = client_groups(folder, windows="windows" in triple)
    notices = {"built": built(version), "groups": natives + crate_groups(manifests, triple)}
    path = os.path.join(folder, "notices.json")
    with open(path, "w", encoding="utf-8", newline="\n") as out:
        json.dump(notices, out, ensure_ascii=False, indent=1)
        out.write("\n")
    print(f"wrote {path}: {len(notices['groups'])} groups, {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    main(sys.argv)
