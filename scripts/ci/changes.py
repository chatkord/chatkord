"""The workspaces a change reaches, read by CI and by the gate (GATE_WORKSPACES): a workspace is reached by a change
to what it builds from, its own directory and every workspace it builds on; a path none names (the gate, the builder's
recipe, CI itself) reaches all, and prose reaches none. The change is the working tree, untracked files included,
against where it left BASE's line. Prints one line of workspace directories, possibly empty.

Usage: changes.py BASE | --all | --self-test
"""
import subprocess
import sys
import tempfile
from pathlib import Path

# What each workspace builds from beside the workspaces it builds on. `platform` embeds the shell's desktop entry and
# icons, the fuzz targets call the client's parsers, and `protocol` is the submodule both build `proto` and `report` from.
BUILDS_FROM = {
    "client": ({"client", "client/shell/assets", "protocol"}, set()),
    "fuzz": ({"fuzz", "protocol"}, {"client"}),
}
WORKSPACES = list(BUILDS_FROM)


def sources(workspace, seen=frozenset()):
    """Every directory `workspace` builds from, through the workspaces it builds on."""
    own, on = BUILDS_FROM[workspace]
    found = set(own)
    for other in on - seen:
        found |= sources(other, seen | {workspace})
    return found


def owner(path):
    """The deepest directory a workspace builds from that holds `path`, None for one no workspace names."""
    named = {source for workspace in WORKSPACES for source in BUILDS_FROM[workspace][0]}
    holding = [source for source in named if path == source or path.startswith(source + "/")]
    return max(holding, key=len, default=None)


def reached(paths):
    """The workspaces `paths` reach, in the gate's order."""
    found = set()
    for path in paths:
        if path.endswith(".md"):
            continue
        source = owner(path)
        found |= {ws for ws in WORKSPACES if source is None or source in sources(ws)}
    return [ws for ws in WORKSPACES if ws in found]


def changed(base, root="."):
    """The paths changed since where HEAD left `base`'s line, untracked ones included; every path while there is no
    commit yet."""
    def git(*args):
        return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout

    if subprocess.run(["git", "rev-parse", "--quiet", "--verify", "HEAD"], cwd=root, capture_output=True).returncode:
        return git("ls-files", "--cached", "--others", "--exclude-standard").split()
    fork = git("merge-base", base, "HEAD").strip()
    return (git("diff", "--name-only", fork) + git("ls-files", "--others", "--exclude-standard")).split()


def self_test():
    cases = {
        "a fuzz target reaches the fuzz workspace alone": (["fuzz/fuzz_targets/invite_link.rs"], ["fuzz"]),
        "a shell asset reaches the client, which embeds it": (["client/shell/assets/chatkord.desktop"], WORKSPACES),
        "the shell's code reaches the client and the fuzz targets": (["client/shell/src/main.rs"], WORKSPACES),
        "the protocol reaches the client and the fuzz targets, which build on it": (
            ["protocol/wire/chatkord/v1/wire.proto"],
            WORKSPACES,
        ),
        "prose reaches none": (["README.md", "client/terms/README.md", "protocol/wire/README.md"], []),
        "a path none names reaches all": (["scripts/gate/gate.sh"], WORKSPACES),
    }
    wrong = [name for name, (paths, want) in cases.items() if reached(paths) != want]
    with tempfile.TemporaryDirectory() as scratch:
        subprocess.run(["git", "init", "--quiet", scratch], check=True)
        Path(scratch, "staged.md").write_text("staged\n", encoding="utf-8")
        subprocess.run(["git", "add", "staged.md"], cwd=scratch, check=True)
        try:
            first = changed("HEAD", scratch) == ["staged.md"]
        except subprocess.CalledProcessError:
            first = False
        if not first:
            wrong.append("a repository with no commit yet, whose every path is a change")
    for name in wrong:
        print(f"changes self-test: wrong answer for {name}")
    if not wrong:
        print("changes self-test: every shape is answered")
    return 1 if wrong else 0


def main(args):
    sys.stdout.reconfigure(newline="\n")
    if args == ["--self-test"]:
        return self_test()
    if args == ["--all"]:
        print(" ".join(WORKSPACES))
        return 0
    if len(args) != 1:
        print(__doc__)
        return 2
    print(" ".join(reached(changed(args[0]))))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
