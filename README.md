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
- `.envrc`: loads the shell through direnv
- `justfile`: common Nix/flake commands
- `.gitignore`: ignores `.direnv/` and `result`

After initialization:

```console
direnv allow  # optional, if you use direnv
nix develop
just --list
```

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
