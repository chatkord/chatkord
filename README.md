# Chatkord

A voice and screen-share chat for people who run their own server: a **native Rust client** (an iced window over a
pinned libwebrtc) for Linux and Windows, with Android, iOS and macOS later, speaking to an **API-only Rust server** with
no hosted interface. It aims to be lighter than the web clients it replaces by removing the layer they are built on,
not rewriting it cheaper. This repository is the client; the protocol both ends speak is its own repository,
`chatkord-protocol`, and the server is `chatkord-server`, which is not open source.

Status: no release yet. A release publishes the client's AppImage and Windows setup on its GitHub Release. The client has the window, voice,
screen share (Linux) with hardware encode and decode, and a command line.

## Where things are

| | |
|---|---|
| `protocol/` | The submodule of `chatkord-protocol`: the protocol's schema (the only place an op exists), the Rust types generated from it, and the checks every repository gates with |
| `client/` | The client's workspace; `client/shell` is the window |
| `scripts/` | The gate, CI's builder and the native dependencies' builds (`scripts/deps`) |

## Building

Rust 1.100.0 through rustup (`rust-toolchain.toml`, with `rustfmt` and `clippy`; the gate calls `rustup`, so its
`~/.cargo/bin` comes before a distribution's `cargo` on `PATH`), `git`, `pkg-config`, `ninja`, `make`, `curl`,
Python 3.11 or later, `ld.lld` (the client's linker) and, for the pinned FFmpeg, `gcc`,
`g++` with a static `libstdc++`, `cmake`, `meson`, `nasm` (on x86-64) and `clang` with its headers. The client also
needs the PipeWire and glib development files (`glib-2.0`, `gobject-2.0`, `gio-2.0`, which the engine's build probes
by name); every other `.pc` comes from `build/deps`, which the dependency scripts write.

- Fedora: `dnf install cmake meson nasm gcc-c++ libstdc++-static clang clang-devel pipewire-devel glib2-devel`
- Ubuntu: `apt-get install meson ninja-build cmake nasm clang libclang-dev lld pkg-config libpipewire-0.3-dev libglib2.0-dev`

The client needs `clang` 21 or newer: the pinned libwebrtc ships Chromium's libc++ headers, and its build refuses an
older compiler, naming both versions. Where the distribution's is older, LLVM's apt script supplies one
(`https://apt.llvm.org/llvm.sh <major>`, then `CC`/`CXX` set to `clang-<major>`, as `.github/workflows/ci.yml` does).
None of this is needed to run the result (the client loads `libpipewire` at run time).

Clone with `git clone --recurse-submodules` (the protocol is `protocol/`; `git submodule update --init` after a plain
clone), and `git config --global submodule.recurse true` so a pull or a branch switch moves it to its pin too; the
gate stops when it is not. Before the first build, `scripts/deps/ffmpeg.sh` (with `scripts/deps/libs.sh` under it), `scripts/deps/glslang.sh` and
`scripts/deps/dxc.sh` build or fetch the pinned FFmpeg and the shader compilers, and `scripts/deps/webrtc.sh` unpacks
the pinned engine crates with our patches and renames the engine's FFmpeg symbols once (`nm` and `objcopy`, from
binutils); a full run of the gate does all three. The gate also needs [mise](https://mise.jdx.dev),
`systemd-run` and a container runtime.

On Windows (x64) the client builds from Git Bash with Visual Studio's C++ build tools (found through `vswhere`), MSYS2
with `make`, `nasm`, `pkgconf` and `diffutils` (`pacman -S make nasm pkgconf diffutils`), LLVM (`libclang`, and
`llvm-nm`, `llvm-objcopy`, `llvm-ar` and `llvm-readobj`, on `PATH` or in LLVM's default install directory),
`pkg-config`, `git` and Python 3.11 or later. `scripts/deps/ffmpeg-windows.sh` builds the pinned FFmpeg as DLLs into
`build/deps/bin`, which a program needs on its `PATH` to start. The gate runs there what Windows can and names what
it leaves to Linux.

## Try the server

The client needs a server to speak to; running one is `chatkord-server`'s own documentation. The client's tests start
the server's test image (`scripts/test-image.sh`).

## Trying the client

In the window (`cd client && cargo run --release -p shell`): paste the invite link you were sent
(`https://example.invalid/join#…`, or the one a new server logs for its first account; the link works with or
without its `https://`, and with `/join/` for `/join#`), check it is the right server, then choose a username and a
password; or type the address of a server you have an account on. The next start
opens that server. On Linux, take a release's AppImage, or build one into `build/package/` with
`scripts/package/linux.sh`: make it executable and keep it at `~/Applications/Chatkord.AppImage`, where the update
replaces it in place, and start it: it adds itself to the launcher (given words, it is the command line:
`Chatkord-….AppImage register …`); a window run from the tree gets its launcher
entry from `scripts/desktop-entry.sh` (a Wayland desktop shows a window's icon only from such an entry). On Windows, a release's
setup, or one `scripts/package/windows.sh` builds into `build/package/`, installs the window for this user, with a
Start menu entry.

From the command line, with an invite code the server's operator made; on Linux the package's one binary takes the same
words (`chatkord member list example.invalid you`, or the AppImage's file in its place):

```
cd client && cargo run -p cli -- register example.invalid you          # a display name may follow; without one others see the username
cargo run -p cli -- sign-in example.invalid you     # no password once the keyring has the token
cargo run -p cli -- channels example.invalid you
cargo run -p cli -- sign-out example.invalid you         # ends this session, on the server as well
cargo run -p cli -- change-password example.invalid you  # a new password, asked after the one in force; ends the other sessions too
```

A call, with the devices it uses (each chosen by the id `devices` lists beside its name; without a choice, the default):

```
cargo run -p cli -- devices                                            # microphones and speakers with their ids, the default first, the chosen ones marked
cargo run -p cli -- use-microphone ID                                  # or --default; the same for use-speaker
cargo run -p cli -- speak example.invalid you CHANNEL                  # --tone sends a test tone; Ctrl-C leaves the channel
cargo run -p cli -- listen example.invalid you CHANNEL                 # hears the channel and says how loud each person is
```

A member who may invite or manage channels (the first account, which joined with a new server's owner code, may do
both) has dev commands until the window does it:

```
cargo run -p cli -- invite create example.invalid you --uses 5 --days 7 --note for-bo   # prints the code once
cargo run -p cli -- invite list example.invalid you
cargo run -p cli -- invite revoke example.invalid you INVITE
cargo run -p cli -- channel create example.invalid you voice "Voice 2"
cargo run -p cli -- channel delete example.invalid you CHANNEL
cargo run -p cli -- invite check example.invalid                 # before signing in: does an invite work?
cargo run -p cli -- member reset example.invalid you MEMBER      # an administrator: prints a temporary password once
cargo run -p cli -- member list example.invalid you              # an administrator: ids, usernames, names, roles
cargo run -p cli -- role list example.invalid you                # ids and names; `member role` takes a name
cargo run -p cli -- member role example.invalid you MEMBER admin give   # or take
cargo run -p cli -- owner transfer example.invalid you MEMBER    # the server owner hands it on (asks the password)
cargo run -p cli -- server picture example.invalid you owl.png  # an administrator: PNG, JPEG or WebP; --none takes it off
cargo run -p cli -- leave example.invalid you                    # deletes your account, asking your password
```

Passwords and invites are asked without echo (`change-password`: the current one, then the new one twice). For a
script, `CHATKORD_PASSWORD`, `CHATKORD_NEW_PASSWORD` and `CHATKORD_INVITE` stand in for the prompts; the same user's
other processes can read an environment variable, so use them only where that is acceptable. The session token is
kept in the system keyring (Secret Service on Linux, Credential Manager on Windows); with none, the password is asked
each time, and a keyring that starts later is used from then on. An address is a host (`example.invalid`) or a host and port; the encrypted form is tried first, plain text
only on this machine, and an address written in full with its socket's scheme is used as written. A server that signs
for itself is trusted by adding its certificate to this device's store.

On Linux with a screen-cast portal and a GPU that encodes H.264, a member shares a screen from the window's call bar
(the share button opens the desktop's picker), and everyone else in the call sees each share in its own card; Pin
fills the stage with one. From a terminal, one member shares a screen and another watches it:

```
cargo run --release -p cli -- share monitor example.invalid you 1        # pick the screen; Ctrl-C stops
cargo run --release -p cli -- watch example.invalid friend 1             # says each second how it arrives; Ctrl-C stops
cargo run --release -p cli -- watch example.invalid friend 1 --stream watch.h264   # keep it as it was sent
```

## Working on it

Pull requests from outside are not taken for now; an issue is welcome.

```
mise run hooks          # once: the git hooks
mise run gate           # the gate, before every push (gate:fast before every commit)
```

## Licence

The client (this repository) is [GPL-3.0-or-later](LICENSE-GPL-3.0.txt). The protocol, in its own repository
`chatkord-protocol`, is MIT OR Apache-2.0; the server is not open source. Each manifest names its licence, and the
About page lists what the client is built with. The name is covered by [TRADEMARKS.md](TRADEMARKS.md); security
reports go as [SECURITY.md](SECURITY.md) says.
