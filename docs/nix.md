# Nix package

Nix can build and install Jcode on Linux, including non-NixOS distributions, and
macOS. Enable the `nix-command` and `flakes` experimental features, or prefix
commands with `nix --extra-experimental-features 'nix-command flakes'`.
Run these commands from this checkout:

```sh
nix build .#jcode
./result/bin/jcode --version
nix run .#jcode -- --help
nix profile install .#jcode
```

The default package includes PDF support but omits the optional Bedrock AWS SDK
and local embedding backend. Other providers and Jev-backed memory do not
need those features. Choose `.#jcode-bedrock` for PDF and Bedrock support, or
`.#jcode-minimal` to omit PDF support too. Install only one variant at a time.
An existing installation can be upgraded with `nix profile upgrade jcode`.

The package disables Jcode's automatic binary updater. Use Nix to manage updates,
not `jcode update`, which still manages the separate `~/.jcode/builds` installation.
An existing `~/.local/bin/jcode` launcher may appear before the Nix profile on
`PATH`. Check `type -a jcode` and run `~/.nix-profile/bin/jcode` explicitly when
needed.

## Installing a remotely built snapshot

Build the same Git revision on the remote machine. Convert its output closure
to content-addressed form before copying it, so the receiving store can verify
it without adding a signing key or disabling signature checks:

```sh
# On the builder; substitute the output paths printed by each command.
nix build .#jcode --no-link --print-out-paths
nix store make-content-addressed /nix/store/<jcode-output>

# On the receiving machine.
nix copy --from ssh-ng://<builder> /nix/store/<content-addressed-output>
nix profile add /nix/store/<content-addressed-output>
```

This installs a pinned snapshot, not a flake-tracking profile entry.
`nix profile upgrade jcode` cannot update it. After rebuilding and copying a new
snapshot, replace that entry with `nix profile remove jcode` followed by
`nix profile add /nix/store/<new-output>`.

Cache keys include source bytes. A fresh clone can differ from a Git-clean
working tree containing CRLF files, so use consistently checked-out sources
when comparing cache reuse between machines.

## Existing servers

Installing the package does not replace other launchers or restart a server.
The TUI can attach to an existing shared server, and automatic server startup can
prefer a binary in `~/.jcode/builds` over the Nix binary. Therefore `--version`
verifies the CLI, not necessarily the server running your session.

To try the Nix server without changing an existing daemon, run it directly on a
separate socket. In one terminal:

```sh
~/.nix-profile/bin/jcode --socket "$XDG_RUNTIME_DIR/jcode-nix.sock" serve
```

In another terminal:

```sh
~/.nix-profile/bin/jcode --socket "$XDG_RUNTIME_DIR/jcode-nix.sock"
```

On systems without `XDG_RUNTIME_DIR`, choose another private socket directory.
Stop the foreground `serve` process when finished.

## Build scope and caching

The package builds only the `jcode` CLI binary, not every workspace member or
test. It uses the repository's compile-time-oriented release profile
(`opt-level = 1`, `codegen-units = 256`), not the release CI's opt-level 3 override.
Crane builds a separate dependency artifact derivation with the same
feature selection as the final binary. Rust source edits can reuse that cache.
Changes to Cargo manifests, the lockfile, features, toolchain, vendored dependency
sources, or native build inputs can require rebuilding dependencies. Each feature
variant has its own dependency cache. The first build still needs to compile
uncached dependencies.
Vendoring the lockfile can fetch crates that the selected feature set will not
compile.

For a bounded local build:

```sh
nix build .#jcode --max-jobs 1 --cores 4
```

`nix flake check --max-jobs 1 --cores 4` runs the packaged CLI's version and help
commands in an isolated home directory without provider credentials.

The result is a native binary for the selected Nix system. It normally refers to
libraries and an interpreter in `/nix/store`, so copying just the binary to a
machine without its Nix closure is not a portable installation. Cross-compiling
and standalone static bundles are not provided by this flake.

The existing development shell is available with `nix develop`.
