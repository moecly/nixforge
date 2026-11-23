# nixforge

`nixforge` is a minimal CLI for initializing a Nix flake development environment.

It does not try to detect languages or generate a dependency matrix. Run it in a project directory, then add the dependencies your project actually needs to `flake.nix`.

## Usage

```console
nixforge init
```

Generated files are only created when they do not already exist. Existing files are never modified or overwritten.

The command can generate:

- `flake.nix`: a minimal multi-platform flake development shell
- `flake.lock`: the input pin shared by all projects (see below)
- `.envrc`: loads the shell through direnv
- `justfile`: common Nix/flake commands
- `.gitignore`: ignores `.direnv/` and `result`

After initialization:

```console
direnv allow  # optional, if you use direnv
nix develop
just --list
```

## Shared flake pin

Without a `flake.lock`, every `nix develop` resolves `nixpkgs-unstable` to its
current revision. Each resolved revision has different store paths, so a new
project downloads and realizes the whole development shell again, and two
projects can silently use different nixpkgs versions.

`nixforge init` therefore also generates `flake.lock` from a shared pin, and
creates that pin on first use from the lock embedded in the binary. New projects
reuse the same nixpkgs revision, so their packages are already in the local
store, and projects do not silently drift to different nixpkgs versions. The pin
lives in `$XDG_DATA_HOME/nixforge/flake.lock` (default
`~/.local/share/nixforge/flake.lock`).

Adopt the lock of an existing project, for example on a new machine:

```console
nixforge pin path/to/old-project/flake.lock
nixforge pin path/to/old-project   # same, reads <dir>/flake.lock
nixforge pin                       # reads ./flake.lock
```

Pin a specific nixpkgs revision, tag, or branch directly:

```console
nixforge pin --rev 93a9ed0a866099a230ed5146a5bb7d16fd5c9b14
```

Refresh the shared pin to the latest nixpkgs:

```console
nixforge update
```

Setting or updating the pin only affects future `nixforge init` calls. Existing
projects keep their own `flake.lock`; update those individually with `just
update` (`nix flake update`) so the change is reviewed like any other commit.

## Adding dependencies

Open `flake.nix` and add packages to `devShells.<system>.default.packages`, or use the generated `forAllSystems` helper as shown in the template:

```nix
packages = with pkgs; [
  just

  # Add project dependencies here.
  nodejs
  python3
];
```

This repository is an example of extending the generated environment manually: its project shell adds Cargo, Clippy, Rustc, and Rustfmt.

## Nix commands

The generated `justfile` intentionally contains only Nix/flake operations:

```console
just develop
just check
just update
just show
just fmt
```

Project-specific build, test, or run commands should be added separately when needed.

## GitHub Actions

The repository includes a GitHub Actions workflow that:

- runs `cargo test --locked` on every push and pull request
- builds release binaries for Linux x86_64, macOS Intel, and macOS Apple Silicon
- uploads the binaries as workflow artifacts
- creates a GitHub Release with those binaries when a `v*` tag is pushed

To publish a release:

```console
git tag v0.1.0
git push origin v0.1.0
```

The binaries can be downloaded from the workflow artifacts or from the GitHub Release page.
