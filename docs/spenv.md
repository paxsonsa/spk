---
title: spenv Usage
chapter: false
---

# spenv: Cascading SPFS Environment Manager

`spenv` lets you define directory-based environment specs (`.spenv.yaml`) that compose into a runtime `/spfs` environment. Specs can:

- Load SPFS layers (`layers:`)
- Include other specs (`includes:` and uptree discovery)
- Configure environment variables (`environment:`)
- Bind host directories into `/spfs` (`contents:`)
- (Optionally) request SPK packages (`packages:`) when built with `--features spk`

`spenv` is secure-by-default: specs do **not** inherit from parent directories unless you opt in.

## Quick Start

From a project directory:

```bash
# Create a new spec
spenv init

# See what would be loaded
spenv show --all

# Enter the environment and run a shell
spenv shell

# Or run a single command in the environment
spenv load -- echo "hello from spenv"
```

This creates `.spenv.yaml` in the current directory and uses it to configure a runtime `/spfs` environment.

## Spec Discovery Model

Given a starting path (`--file` / `-f`, default `"."`):

1. **CLI includes** (`-i / --include`) are loaded first.
2. **Env var includes** from `SPENV_INCLUDE` (colon-separated list) are loaded next.
3. **In-tree specs**:
   - Looks for `.spenv.yaml` at the start path.
   - If found, it’s loaded.
   - If inheritance is enabled (see below), it walks up parent directories, loading parent `.spenv.yaml` files until it reaches one with `inherit: false` or the filesystem root.
4. **Local override**:
   - If `.spenv.local.yaml` exists at the start path, it’s loaded last as a local override.

### Inheritance Controls

- In the spec:

  ```yaml
  api: spenv/v0
  inherit: false  # default (secure)
  ```

- CLI and env override the file:

  - `spenv show --inherit` or `spenv load --inherit`
  - `SPENV_INHERIT=1`
  - `spenv show --no-inherit` or `spenv load --no-inherit`
  - `SPENV_NO_INHERIT=1`

If inheritance is disabled (`inherit: false` and no override), only the current directory’s `.spenv.yaml` is used.

## `.spenv.yaml` Schema

Minimal example:

```yaml
api: spenv/v0

# Optional human description
description: "My project environment"

# Inheritance from parent .spenv.yaml files (default false)
inherit: false

# Explicit includes
includes:
  - ~/.config/spenv/defaults.spenv.yaml
  - /team/shared/base.spenv.yaml

# SPFS layers (tags or digests)
layers:
  - platform/centos7
  - dev-tools/latest

# Environment operations
environment:
  - set: PROJECT_ROOT
    value: /spfs/project
  - prepend: PATH
    value: /spfs/bin
  - append: LD_LIBRARY_PATH
    value: /spfs/lib
  - comment: "extra tools"
  - priority: 50

# Bind mounts into /spfs
contents:
  - bind: ./src
    dest: /spfs/project/src
    readonly: false

# Optional SPK packages (requires spenv built with --features spk)
packages:
  - python/3.11
  - cmake/3.26

package_options:
  binary_only: true
  repositories: []        # reserved for future use
  solver: resolvo         # or "step" (default)
```

### Fields

- `api`: must be `spenv/v0`.
- `description`: free-form string.
- `inherit`: `false` by default; when `true`, discovery walks parents.
- `includes`: list of other `.spenv.yaml` paths:
  - Supports absolute, relative (to the spec’s directory), and `~/...`.
- `layers`: ordered list of SPFS layer refs (tags or digests).
- `environment`: ordered list of env ops:
  - `set: VAR` / `value: ...` → `VAR=value`.
  - `prepend: VAR` / `value: ...` → `VAR="value:…:$VAR"` (or `;` on Windows).
  - `append: VAR` / `value: ...` → `VAR="$VAR:…"` (or `;` on Windows).
  - `comment: "text"` → `# text` in script.
  - `priority: N` → influences script filename ordering (e.g. `50_spenv.sh`).
- `contents`: bind mounts:
  - `bind`: host path (relative to spec dir, absolute, or `~/`).
  - `dest`: target in `/spfs` (e.g. `/spfs/project/src`).
  - `readonly`: currently advisory; mounts are configured by SPFS.
- `packages` / `package_options`:
  - Only used when `spenv` is compiled with `--features spk`.
  - `packages` is list of SPK package idents (e.g. `maya/~2020`, `python/3.11`).
  - `binary_only` defaults to `true` to avoid source builds.
  - `solver` selects `step` (default) or `resolvo`.

## CLI Commands

### `spenv init`

Create a `.spenv.yaml` in the target directory:

```bash
spenv init                # standard template in .
spenv init path/to/dir    # create in another directory

# common flags:
spenv init --inherit          # set inherit: true in template
spenv init --layer base/env   # seed layers section
spenv init --template minimal # or full
```

Outputs a commented template with guidance and example fields.

### `spenv show`

Display discovered specs and the composed environment:

```bash
spenv show              # default: files + layers
spenv show --files      # only discovery order
spenv show --layers     # only merged layer list
spenv show --all        # files + layers + env (if present)
```

Useful flags:

- `--file/-f PATH`: start path (dir or `.spenv.yaml`).
- `--inherit` / `--no-inherit`: override in-tree inheritance.
- `-i, --include PATH`: extra `.spenv.yaml` to include.
- `--format table|yaml|json`:
  - `table` (default) shows pretty columns with colors.
  - `yaml` prints a simple YAML view of layers/environment.
  - `json` prints machine-readable summary.

Environment variables:

- `SPENV_INCLUDE=spec1.yaml:spec2.yaml`
- `SPENV_INHERIT=1` / `SPENV_NO_INHERIT=1`

### `spenv load`

Compose the environment and run a command in it:

```bash
# Run a shell (default: $SHELL or /bin/bash)
spenv load

# Run a single command
spenv load -- echo "hello"

# Dry-run: don’t enter runtime, just show what would be loaded
spenv load --dry-run
```

Key flags:

- `--file/-f PATH`, `--inherit`, `--no-inherit`, `-i/--include`: same as `show`.
- `--edit`: make runtime editable.
- `--keep`: keep runtime after exit (named runtimes are easier to rejoin).
- `--name NAME`: runtime name.
- `--dry-run`: preview discovered files and layers.

`spenv load`:

1. Discovers specs.
2. Composes layers, environment, contents, and packages.
3. Creates an SPFS runtime:
   - Applies `layers` as base stack.
   - If compiled with `spk` and `packages` present, resolves and applies SPK packages.
   - Generates an env startup script layer from `environment`.
   - Adds live-layer bind mounts from `contents`.
4. Uses `spfs-enter` to execute your command.

### `spenv shell`

Convenience wrapper around `spenv load` for an interactive shell:

```bash
spenv shell                 # uses $SHELL or /bin/bash
spenv shell --shell zsh     # use a specific shell
```

Flags mirror `spenv load` plus:

- `--shell SHELL`: override shell binary.

### `spenv lock` and `spenv check`

Lock files capture the resolved environment (sources + layer digests) for CI and reproducibility.

**Generate/update lock:**

```bash
spenv lock           # creates .spenv.lock.yaml next to .spenv.yaml
spenv lock --update  # update existing lock
spenv lock --force   # regenerate regardless of existing file
spenv lock --check   # verify and exit 0/1/2
```

Behavior:

- `spenv lock` without `--update`/`--force` refuses to overwrite an existing lock.
- `spenv lock --check`:
  - Exits `0` if lock matches current env.
  - Exits `1` if out of date.
  - Exits `2` if lock missing.

**Verify in CI:**

```bash
spenv check                    # warn on mismatch; exit 0/2
spenv check --strict           # treat mismatch as error; exit 1
```

- Loads `.spenv.lock.yaml`, recomputes current source hashes and layer digests, and reports differences.
- Strict mode is suitable for CI to enforce “no drift”.

## Typical Workflows

### Per-project environment

1. In your repo root:

   ```bash
   spenv init
   ```

2. Edit `.spenv.yaml` to add `layers`, `environment`, and `contents`.
3. In development:

   ```bash
   spenv show --all
   spenv shell
   ```

4. In CI:

   ```bash
   spenv lock         # done once and committed
   spenv check --strict
   ```

### Shared base + project override

- `~/.config/spenv/base.spenv.yaml`:

  ```yaml
  api: spenv/v0
  inherit: false
  layers:
    - tools/base
  ```

- Project `.spenv.yaml`:

  ```yaml
  api: spenv/v0
  inherit: false
  includes:
    - ~/.config/spenv/base.spenv.yaml
  layers:
    - project/layers
  ```

- Use:

  ```bash
  spenv show --all
  spenv shell
  ```

### Package-driven environment (with `spk` feature)

When `spenv` is compiled with `--features spk`:

```yaml
# .spenv.yaml
api: spenv/v0

packages:
  - python/3.11
  - maya/~2020

package_options:
  binary_only: true
  solver: step
```

Then:

```bash
spenv load --dry-run      # preview package layer resolution
spenv load -- echo "python --version"
```

Packages are resolved from the local SPK repository (`spk-storage::local_repository`) and their layers added to the runtime stack through `spk-exec`.

## Repository Selection

When `spenv` is built with `--features spk`, you can control which SPK repositories are used for package resolution using CLI flags and environment variables.

### Default Behavior

By default, `spenv` uses:
- **local** repository (your local SPK storage)
- **origin** repository (if configured in SPFS config)

### Repository Flags

Available on commands that resolve packages (`load`, `shell`, `lock`, `check`):

```bash
# Enable additional repositories
spenv load --enable-repo staging
spenv load -r staging -r prod

# Disable specific repositories
spenv load --disable-repo origin

# Use only local repository
spenv load --local-repo-only

# Disable local repository (remote only)
spenv load --no-local-repo --enable-repo origin
```

### Environment Variables

Repository flags can also be controlled via environment variables:

- `SPENV_ENABLE_REPO=staging` (multiple values not supported via env)
- `SPENV_DISABLE_REPO=origin`
- `SPENV_NO_LOCAL_REPO=1`
- `SPENV_LOCAL_REPO_ONLY=1`

CLI flags override environment variables.

### Examples

**Use staging repository for testing:**
```bash
spenv load --enable-repo staging -- python --version
```

**Prevent network access (local only):**
```bash
spenv load --local-repo-only
```

**Skip origin, use custom remote:**
```bash
spenv load --disable-repo origin --enable-repo prod
```

**Note**: Repository names must be configured in your SPFS config (`~/.config/spfs/config.toml`). The `package_options.repositories` field in `.spenv.yaml` is not currently used; control repositories via CLI/env only.
