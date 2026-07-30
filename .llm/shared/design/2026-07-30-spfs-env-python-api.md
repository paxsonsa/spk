---
date: 2026-07-30T00:20:00-0700
repository: spk
git_commit: 66aa0c9a
branch: worktree-spfs-discovery-doc
title: "spfs env tool suite: config store and Python API"
status: draft
tags: [design, spfs, ilm, environment, python, api, annotations]
related:
  - 2026-07-29-ilm-env-hierarchy-spec.md
  - 2026-07-29-spfs-env-user-guide.md
---

# The spfs env tool suite: config store and Python API

> ⚠ **STRESS-TESTED — the read path this is built on is inverted, and several write claims are WRONG. Read [`2026-07-30-spfs-env-stress-test-findings.md`](./2026-07-30-spfs-env-stress-test-findings.md).** The "blessed read path" premise (`all_annotations()` = highest-wins) is only true for a flat stack; with nested platforms (which the design mandates) traversal is BFS and the deepest-nested annotation wins, inverting precedence — so the reader must be a custom top-down stack walk, not either built-in (C1/D11). Confirmed wrong: `cfg.set()` is O(n) per write with unbounded growth, not "cheap" (D2); `batch()` is not atomic and an empty batch hard-errors (§3 sub-findings); reads silently return `default` on storage faults (D3); tombstones leak into published platforms via `commit_platform` and resurface stale values (D5); `env.digest`/`env.runtime_stack_digest` are unimplementable/unstable as specified (D6). "Nothing upstream tests duplicate keys" is false (`storage_test.rs:110-124`). The suite *layering* (Rust core / CLI / subprocess-then-PyO3) is sound; the semantics need the findings applied.

Design for the tool suite around hierarchical environments, centered on the piece programs touch most: reading and writing configuration (annotations) from inside a runtime, from Python.

## TL;DR

- **One brain, many mouths.** A single Rust core crate owns resolution, config composition, and provenance. The CLI, the Python package, and any future C API are thin frontends over it. This is the workspace's own pattern: `crates/spk/src/lib.rs` is 13 lines over library crates.
- **A blessed read path is mandatory, not nice-to-have.** The two annotation readers in spfs disagree on precedence, and the shipping CLI exposes *both*: `spfs info --get KEY` returns the **lowest** layer's value, `spfs info --get-all` returns the **highest**. Same runtime, same key, two answers. No program should ever touch raw annotations for config.
- **Python API v1 is a subprocess backend behind a stable interface**; v2 swaps in PyO3 native bindings without changing caller code. No Python bindings exist anywhere in the workspace today (no pyo3, no maturin) — this is greenfield.
- **Set is runtime-scoped and ephemeral.** Persistent config changes go through spec files and the publish pipeline. The API sets annotations on the *running* runtime only: job metadata, checkpoints, discovered values. This keeps the write story simple and the trust story intact.
- **Batch writes are a correctness feature.** `add_annotation` in a loop creates one layer per key. The API's batch context manager exists so fifty keys cost one layer, not fifty.
- **Unset is implementable even though annotations can't unset**, because we own the read path: a tombstone sentinel that `get` translates to "absent."

## 1. Suite layout

```
crates/spfs-env-core/     Rust: resolution engine, config compose, provenance,
                          lock, schema validation. The only place with logic.
crates/spfs-env/          Rust bin `spfs-env`: clap over core. Ships `spfs env`
                          via external-subcommand dispatch, zero spfs changes.
python/spfsenv/           Python package. v1: subprocess backend over the CLI's
                          versioned --json output. v2: native ext via PyO3/maturin.
                          Same interface both ways.
(later) spfs-env-capi     cbindgen C header over core, for compiled DCC plugins
                          that can't reach embedded Python. Not v1.
```

Rules that keep this honest:

1. **The CLI has no logic the core doesn't.** Anything the CLI can print, the core exposes as a typed function, so the native Python backend gets it for free later.
2. **JSON output is a versioned contract** (§7). The subprocess backend depends on it; it changes additively or with a version bump, never silently.
3. **Python never parses human-readable output.** Every command the backend uses has `--json`.

## 2. Why the blessed read path is mandatory (evidence)

Verified against the source, current as of this branch:

| Reader | Walk | Winner on duplicate key | Used by |
| --- | --- | --- | --- |
| `Runtime::annotation(key)` | bottom-up, first match | **lowest** layer | `spfs info --get`, **spk itself** (`spk-cli/common/src/env.rs:94` reads `spk_solve` this way) |
| `Runtime::all_annotations()` | bottom-up, BTreeMap insert | **highest** layer | `spfs info --get-all` |

So the shipping surface already disagrees with itself, nothing upstream tests duplicate keys, and even spk's own read path is on the lowest-wins branch — safe today only because `setup_runtime` *replaces* the stack, so a second `spk_solve` annotation never coexists with the first. The moment our resolver puts annotation-bearing layers into stacks that spk didn't build, that assumption is dead.

The suite's answer, restated from the user guide and now load-bearing:

- The resolver composes all `config:` sections at resolve time into **one merged annotation** (`ilm.cfg.merged`, serialized as canonical JSON) on a synthetic layer at the top of the resolved stack. Per-source values live beside it (`ilm.cfg.sources`) for provenance.
- Runtime-scope `set` pushes annotation layers **above** that.
- The blessed read: overlay runtime-set keys (top-wins among themselves) onto the merged blob. One rule, stated once: **later set wins; above that, higher slot wins, deeper level wins.**
- `spfs env config …` and `spfsenv` (Python) are the only supported readers. `spfs info --get` on `ilm.cfg.*` keys is explicitly unsupported for programs.

An upstream contribution should still add the duplicate-key test and a doc warning on the two readers, but the suite does not wait on it.

## 3. The Python API

Package name `spfsenv`, importable inside and outside runtimes.

```python
import spfsenv

# ---- inside a runtime -------------------------------------------------
env = spfsenv.current()                  # raises NotInRuntime outside one
env.name                                 # runtime name (SPFS_RUNTIME)
env.digest                               # platform digest (identity)
env.reproducible                         # False if any live bind mount
env.sources()                            # provenance: slot, ref/path, digest/hash

cfg = env.config
cfg.get("show.colorspace")                       # -> str | None
cfg.get("app.nuke.gpu", default="auto")
cfg.get("review.burnin", cast=bool)              # schema-aware cast (§6)
cfg["show.colorspace"]                           # KeyError if absent
cfg.items(prefix="app.nuke.")                    # iterator of (key, value)
cfg.sources("show.colorspace")                   # [(value, source), ...] full chain

cfg.set("job.checkpoint", "frame_0042")          # runtime scope, ephemeral
with cfg.batch() as b:                           # MANY KEYS -> ONE LAYER
    b.set("job.id", job_id)
    b.set("job.submitted_by", user)
    b.unset("job.retry_hint")                    # tombstone (§4.3)

# ---- outside a runtime (submitters, services, launchers) -------------
res = spfsenv.resolve("/show/abc/tst/tst0100/comp",
                      published_only=True, app="nuke")
res.digest; res.sources; res.solve               # Resolution dataclass
res.config.get("show.colorspace")                # config WITHOUT entering

proc = spfsenv.run(res.digest, ["nuke", "-x", "shot.nk"],
                   env_overrides={"FOO": "1"})   # subprocess.Popen semantics
```

### 3.1 Semantics table

| Operation | Behavior |
| --- | --- |
| `get(key)` | Blessed precedence (§2). Returns `str` unless `cast`/schema says otherwise. Absent → `default` (default `None`). Tombstoned → absent. |
| `__getitem__` | Same, but `KeyError` on absent. |
| `items(prefix=)` | Merged view, tombstones filtered. |
| `sources(key)` | Full chain lowest→highest including runtime sets and tombstones, for `--explain`-grade debugging from Python. |
| `set(key, value)` | One annotation layer on the *runtime* stack. Value coerced to `str`; non-str rejected (`TypeError`) rather than repr'd silently. |
| `batch()` | Context manager; all sets/unsets in one layer via `add_annotations` (plural). Atomic: either the layer lands or nothing does. |
| `unset(key)` | Writes tombstone sentinel; blessed readers treat as absent. Raw readers see the sentinel — documented, deliberate. |
| `refresh()` | Drops the process-local cache (§3.2). |

### 3.2 Caching and cost

v1 backend is subprocess (`spfs env config dump --json`), so the design assumes one call, not many: first `get` loads the full merged config plus sources into a process-local cache; subsequent gets are dict lookups. `set`/`batch` invalidate. A DCC session does **one** subprocess spawn at startup, not one per key. `sources()` and `resolve()` are separate calls, also cached by input.

Staleness rule: the cache is per-process and invalidated only by this process's writes or explicit `refresh()`. Another process's `set` is invisible until then — documented, because config is not an IPC mechanism (§4.2).

### 3.3 Errors

Small, typed hierarchy: `SpfsEnvError` → `NotInRuntime`, `ConfigWriteError` (carries the Legacy-encoding case with the actual fix in the message), `ResolveError` (carries solver explanation text), `ContractError` (CLI JSON version mismatch — the skew guard).

## 4. Write semantics and their sharp edges

### 4.1 What a set actually does

`cfg.set` → core calls `Runtime::add_annotation(s)` → new annotation-only layer pushed on the runtime stack → `save_state_to_storage()`. **No remount**: annotation layers carry no manifest, so the filesystem is untouched; readers load runtime state from storage and see the new layer immediately. This is why runtime-scope config writes are cheap and safe while file changes are not.

Consequences worth stating plainly:

- **Scope is the runtime's lifetime.** Transient runtime → gone at exit. Durable runtime → persists across reruns. Both correct; the API exposes `env.durable` so tools can tell.
- **Writes change the runtime stack, not the published platform.** The resolved environment's identity (`res.digest`) is unchanged; `env.digest` reports the resolved platform, and `env.runtime_stack_digest` (distinct property) reflects runtime-added layers. Committing a platform from inside (`spfs commit platform`) *would* include set annotations — flagged in docs as the one path where runtime sets leak into published identity.
- **Legacy encoding blocks all writes** (`storage.encoding_format = "Legacy"`). Surfaces as `ConfigWriteError` at the first write with the config fix named, not as a mystery at read time.

### 4.2 Concurrency: last-writer-wins, on purpose

Runtime state is saved via tag pushes with no compare-and-swap. Two processes in one runtime writing config concurrently race, and the loser's annotation layer can be dropped from the saved stack. The suite does not fix this; it scopes it: config is **settings, not coordination**. Job metadata written once at startup, checkpoints written by a single owner process — fine. High-frequency shared state — use a real store. The API docs say exactly this, and `batch()` narrows the window to one save.

### 4.3 Unset via tombstone

Raw annotations cannot unset (append-only layers). But §2 gives us an owned read path, so: `unset` writes value `"\x00ilm.tombstone\x00"`; blessed readers translate to absent; `sources()` still shows the tombstone event for debugging. Schema linting (§6) rejects the sentinel as a *literal* value in spec files so it can't be published by accident.

## 5. Backend strategy

### v1: subprocess

Ships with the CLI, zero binding maintenance, works wherever the binary does, and the caching model (§3.2) makes spawn cost irrelevant for the dominant read pattern. Requirements it imposes: `spfs-env` on PATH (facility-managed), `--json` on every consumed command, contract version in every payload.

### v2: native (PyO3 + maturin), same interface

`spfsenv._native` compiled against `spfs-env-core`; the Python package picks the backend at import (`SPFSENV_BACKEND=cli|native` to force). Callers never change. What native buys: no spawn (relevant for `resolve()` in hot submitter loops), typed structs end-to-end, and in-process access to provenance without JSON round-trips. What it costs: a build matrix (Python versions × platforms — facility-controlled, so small), and embedding a tokio runtime inside a Python extension (`pyo3-asyncio` or a private runtime handle; core APIs stay sync-facing to Python). The interface contract in §3 is written so nothing in it presumes either backend.

### Not chosen: pure-Python repo reader

Reading flatbuffers objects and walking stacks from Python directly couples every Python deploy to the storage format and reimplements precedence — the exact bug class this suite exists to kill. Rejected on principle.

## 6. Schema integration

The schema registry (env-config git repo, per the user guide §3.4) is not just CI lint — it feeds the API:

```python
cfg.get("review.burnin", cast=bool)     # cast validated against schema type
spfsenv.schema()                        # {key: {type, owner, doc}} for tool UIs
cfg.set("show.colorspace", "rec709")    # warns (or raises, per policy) on
                                        # unknown key / type mismatch
```

Typed gets mean the string-only substrate stops leaking into every call site (`cast=bool` instead of forty copies of `== "on"`), and `spfsenv.schema()` gives pipeline UIs introspection (settings dialogs, validation) from the same source of truth CI uses. Unknown-key policy is config: warn in dev, strict on farm.

## 7. JSON contracts

Every payload carries `contract: 1`. The subprocess backend refuses a major it doesn't know (`ContractError`), which turns silent version-skew corruption into a clean error naming both versions.

```json
// spfs env config dump --json
{"contract": 1,
 "digest": "LQ7XN...====",
 "merged": {"show.colorspace": "acescg", "review.burnin": "on"},
 "sources": {"show.colorspace": [
    {"value": "aces", "source": "ilm/base", "slot": 1},
    {"value": "acescg", "source": "ilm/show/abc", "slot": 2}]},
 "runtime_sets": [{"key": "job.checkpoint", "value": "frame_0042",
                   "layer": "QW3E...", "tombstone": false}]}
```

```json
// spfs env resolve PATH --json   (same payload the farm submitter consumes)
{"contract": 1, "digest": "R8YWK...====",
 "reproducible": true,
 "sources": [...], "solve": {"packages": [...]}, "warnings": []}
```

Additive fields are free; renames/removals bump the major. The contract doc lives next to the core crate and is the *only* coupling surface between Python v1 and Rust.

## 8. Worked examples

**Nuke init.py** — one spawn, then dict lookups:

```python
import spfsenv, nuke
cfg = spfsenv.current().config
nuke.knobDefault("Root.colorManagement", "OCIO")
if cfg.get("review.burnin", cast=bool, default=False):
    nuke.pluginAddPath(cfg["review.burnin_gizmo_path"])
```

**Farm submitter** — resolve once, tag for retention, stamp the job:

```python
res = spfsenv.resolve(shot_path, published_only=True, as_of=batch_pin)
if not res.reproducible:                # bind mounts present
    raise SubmitError(res.warnings)
spfsenv.tag(res.digest, f"jobs/{job_id}")
job.env_digest = res.digest
```

**Worker-side checkpointing** — the canonical runtime-scope write:

```python
env = spfsenv.current()
with env.config.batch() as b:
    b.set("job.frame_done", str(frame))
    b.set("job.host", socket.gethostname())
```

## 9. Build order

1. Core: merged-config compose + blessed reader, with the duplicate-key precedence tests upstream never wrote. This is the keystone; everything reads through it.
2. CLI: `config dump/get/set/list --json` against a hand-assembled runtime (no resolver needed yet — annotations work in any runtime today).
3. Python package, subprocess backend, `current()` + `config` only. Ship to one DCC integration to shake the interface.
4. `resolve()`/`run()`/`tag()` once the resolver (design doc build order 1–3) lands.
5. Schema-aware casts + `spfsenv.schema()`.
6. PyO3 backend when spawn cost or submitter throughput actually demands it — measured, not assumed.

Steps 1–3 are useful *before* the hierarchy resolver exists: `spfs run --annotation`-launched runtimes plus the Python read/write API is already a shippable improvement over parsing `spfs info` output.

## 10. Open questions

- **`resolve()` without entering: where does config come from?** Pre-resolution config lives in the merged annotation of the *resolved platform*, so `res.config` reads objects from the repo without a runtime. Needs a core path that reads annotations off a platform digest directly (no `Runtime`) — straightforward, but it's new core surface to design deliberately.
- **Watch/subscribe?** Deliberately absent (config is not IPC), but DCC session-long processes may want "re-read on next file-open" hooks. Revisit after real usage, not before.
- **The C API trigger.** Which compiled integrations (if any) can't reach embedded Python? Survey before building `spfs-env-capi`; it may never be needed.
- **spk interop for the `spk_solve` key.** When the resolver owns the solve, should it write `spk_solve` in spk's format so `spk` CLI introspection keeps working inside resolved runtimes? Probably yes; needs a compatibility test against `spk-cli/common/src/env.rs:94`'s lowest-wins read (one `spk_solve` annotation only, ever).
