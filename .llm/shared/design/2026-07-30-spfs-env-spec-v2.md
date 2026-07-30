---
date: 2026-07-30T01:14:12-0700
repository: spk
git_commit: 3d1afd65
branch: worktree-spfs-discovery-doc
title: "spfs env: hierarchical environment resolution — spec v2"
status: authoritative
tags: [design, spfs, ilm, environment, spec, v2]
supersedes:
  - 2026-07-29-ilm-env-hierarchy-spec.md
incorporates:
  - 2026-07-30-spfs-env-stress-test-findings.md
  - 2026-07-30-spfs-env-decisions.md
---

# spfs env — spec v2

Single build-from specification for the path-driven environment resolver. This is v1's design ([`2026-07-29-ilm-env-hierarchy-spec.md`](./2026-07-29-ilm-env-hierarchy-spec.md)) rebuilt with the [stress-test findings](./2026-07-30-spfs-env-stress-test-findings.md) applied and the four [owner decisions](./2026-07-30-spfs-env-decisions.md) folded in. Where v1 conflicts with this, **this wins**. Every code fact cited was verified against `crates/spfs` and `crates/spk*` during the stress test; file:line references point at the constraint, not a suggestion.

## What changed from v1 (orientation for anyone who read v1)

- Env composition is **owned by the resolver end to end**, and it **masks `/spfs/etc/spfs/startup.d`** — because startup.d runs *after* the resolver's overrides and spk bakes per-package env ops into it (C2).
- The config store **never uses `all_annotations()`/`annotation()`** — both are breadth-first and invert precedence on nested platforms. The resolver walks the stack itself (C1).
- Resolution is **host-qualified by default** (os/arch/distro in the identity), with `--no-host` and per-layer `agnostic: true` opt-outs (D-C / C3).
- **spk packages and promoted layers are separate mechanisms** (D-A); promoted content is loudly *unmanaged*.
- **Resolution never auto-tags.** Runtimes are ephemeral by default; `spfs env commit` is the only reproduce-by-digest guarantee (D-D / C5). Step 14 of v1 is deleted.
- Mapping-layer dests are `/spfs`-relative and **stripped** (v1 examples produced `/spfs/spfs/...`), mappings are file-granularity, and parent-dir modes are explicit (C6).
- Context vars are **validated component-by-component after substitution**, and the resolver builds its **own env-proofed `spfs::Config`** (S1-S3).

## Goals (unchanged)

Hierarchical layering; directory-level (asdf-style) environments; point-in-time launch; cheap ad-hoc override with a promotion path; env vars + key/value config composed along the hierarchy; file mapping; spk packages as a first-class source; pin/lock with change alerts.

---

## 1. Data model: three declaration kinds, three composition rules

A spec file carries three kinds of declaration that **do not compose the same way**. This is the spine of the whole design.

| Kind | Fields | Rule | Conflict resolves |
| --- | --- | --- | --- |
| **Stacked** | `layers`, `mappings`, `mask` | positional, higher shadows lower | silently (filesystem last-wins) |
| **Accumulated** | `requests`, `replaces` | merged across levels, solved **once** | loudly (solve failure with explanation) |
| **Composed** | `env` | applied in band order | deterministic override |
| *(data)* | `config` | merged, resolver-owned read path | higher wins (see §6) |

Rule 2 is loud because spk **intersects** version ranges: "show wants maya/2024" + "shot wants maya/2023" is unsatisfiable, not an override — `replaces` is the only way to express override, and it must run before the solve. Prefer `requests:` over `layers:` for anything with dependencies (D-A).

---

## 2. File format

```yaml
api: ilm/v0/env            # REQUIRED. No default. A file without this is a hard error.
inherit: true              # default true; false stops the upward crawl at this level

# --- Stacked (Rule 1) ---

layers:                    # existing SPFS refs. tags follow, digests pin.
  - ilm/sww/gfx/2026.06
  - GXT4A...====           # digest = hard pin

mappings:                  # place committed content at a path. file-granularity.
  - src: ilm/tools/fxutils/1.4.0   # a REF (tag/digest) already in spfs, not a host path
    pick: [bin/mytool, lib/libmytool.so]   # specific files; whole-tree mapping is gated
    dest: /spfs/app/mytool/1.4.0           # /spfs-relative; the /spfs prefix is stripped
    agnostic: false                        # default false = host-qualified

mask:                      # hide paths from everything BELOW this level
  - /spfs/bin/legacy-comp

# --- Accumulated (Rule 2) ---

requests:                  # spk requests. merged across all levels, solved ONCE.
  - nuke/15.1
  - ilm-comp-tools/3
replaces:                  # drop inherited requests for these packages before the solve
  - nuke

# --- Composed (Rule 3) ---

env:                       # composed in band order (§5). set/prepend/append.
  - prepend: PATH
    value: /spfs/app/nuke/15.1/bin
  - set: OCIO
    value: /spfs/etc/ocio/config.ocio

# --- Data ---

config:                    # key/value, carried as annotations, read via §6
  show.colorspace: acescg

apps:                      # per-application fragments, merged only with --app <name>
  maya:
    requests: [mtoa/5.4]
    env: [{prepend: MAYA_MODULE_PATH, value: /spfs/show/abc/maya/modules}]

# --- Dev only. Rejected at publish time. ---
bind:
  - src: ./work            # live host bind mount; must resolve under this file's dir
    dest: /spfs/mystuff
```

### Field notes (only where v2 differs or a code fact constrains)

- **`api:` has no default.** v1 relied on `SpecApiVersion::default()`, which meant a missing `api:` parsed as a live layer and then failed confusingly on a `bind:` entry the author never wrote (`env.rs:46-49`, `:102-113`). The resolver's own parser requires `api:` explicitly and never routes its files through spfs's `SpecFile::parse` (wrong suffix anyway — spfs wants `.spfs.yaml`, ours is `.spfs-env.yml`, B7).
- **`mappings.src` is a spfs ref, not a host path.** v1's "host path, zero copy" was unimplementable — spfs has no host-path→digest index (C6). `src` names already-committed content (a tag or digest); `pick` selects entries from its manifest; the resolver builds a metadata-only layer re-rooting those entries' existing blob digests. Whole-tree mappings (`pick` omitted) are gated behind config (`allow_tree_mappings`) because a new manifest = a new render + N inodes/user (C6).
- **`mappings.dest` / `mask` paths are `/spfs`-relative and the `/spfs` prefix is stripped** before building manifest paths (manifest paths are already rooted at `/spfs`; v1 examples produced `/spfs/spfs/...`, C6). Parent directories synthesized by re-rooting get an explicit mode from config (default `0o755`, **not** the `0o40777` that `mkdirs` uses, C6), and that mode is part of the layer's canonical form so identical mappings dedupe.
- **`agnostic: true`** (per layer/mapping) asserts no os/arch dependence; excludes the entry from the host-qualified digest partition (§4, D-C). Default false.
- **`bind:`** maps to spfs live layers (`src` confined under the spec file's dir, `live_layer.rs:33-50`) and is **rejected at publish** (a bind mount can't be shared) and makes the runtime non-reproducible (§7).
- **Two lifecycles:** a published spec (compiled by CI → a tag) and a discovered spec (read live on disk). `inherit` and `bind` are resolution-time-only; a publish path rejects `bind` and bakes `inherit` resolution in.

---

## 3. Source slots and context

Four slots, fixed order, bottom to top. Slots 1-2 are trusted (writing them needs governed repo access) and time-travelable; slots 3-4 are neither.

| # | Slot | Origin | Trust |
| --- | --- | --- | --- |
| 1 | Base | system config (`ilm/base`) | root-owned config |
| 2 | Tag | path/context-derived tag probe | governed repo write |
| 3 | Discovered | `.spfs-env.yml` crawled up from cwd | allowlist + ownership (§8) |
| 4 | Personal | `$HOME`, `--with`, `$SPFS_ENV_WITH` | user's own |

Slots 2 and 3 are **depth-interleaved**: at each level the tag applies, then the discovered file at that same level. A file at level N beats the tag at level N and everything above; loses to the tag at level N+1.

### Context (drives slot-2 candidates)

Resolution is a function of a validated **context** — a small map of variables from ordered providers.

```toml
# /etc/spfs-env/config.toml  (root-owned; NOT spfs config; NO env override — §8)
[context.vars.show]
from = ["flag", "env:SHOW", "path"]     # precedence flag > env > cwd extraction
pattern = "^[a-z0-9][a-z0-9_-]*$"       # strict; see validation below

[context.path]
patterns = ["/show/{show}/{seq}/{shot}", "/show/{show}"]

[probe]
candidates = ["ilm/base", "ilm/show/{show}", "ilm/show/{show}/{seq}", "ilm/show/{show}/{seq}/{shot}"]
```

**Validation is mandatory and component-wise (S1/S2/B1/B2).** Substituted values land in tag names, and the tag charset allows `.` and (in org position) `/`, so a naive check lets `..` and `/` through and `split_tag_spec` will not catch it. Rules:

1. Reject `/` in any substituted value unconditionally (a `/` restructures the whole candidate).
2. Reject any value whose slash-components equal `.` or `..`.
3. Reject empty values (set-but-empty `$SHOW` otherwise collapses `ilm/show//tst` → the tag of a show *named* tst, S2).
4. **Re-validate the fully assembled candidate string component-by-component after substitution**, and confirm it re-serializes to itself (catches `~N` version-suffix misparse, B2, and `//` aliasing).
5. The resolver validates candidates itself and **never hands un-prescreened bytes to `split_tag_spec`** — its error path panics on non-ASCII (char-index-as-byte-index, B1).

**Provider conflict (Decision 7):** on an interactive tty, if `env:SHOW` and path-extracted `show` disagree, **hard error** by default (the stale-`$SHOW`-from-Tuesday ticket); env-wins is reserved for non-tty/flags. Configurable per facility. `--pure` ignores env providers entirely.

### Probe

Probe **all** candidate levels in parallel; gaps are legal. But classify errors: only `ENOENT` is a cheap miss (`error.rs:325-338` — `is_os_not_found` matches only ENOENT); `ENAMETOOLONG` (long level names) and `EACCES` (per-show tag ACLs) are **hard failures**, not skipped gaps (B3). Paths that can't map to a legal tag (spaces, `+`, non-ASCII) resolve via a **config-declared alias map** (reviewed, injective) rather than sanitizing (collisions) or hard-erroring (dead directories) (B-charset).

---

## 4. Host qualification and identity (D-C / C3)

The platform digest is a pure function of the stack, but the stack is not a pure function of the recorded inputs unless host state is captured. It is.

**Default: host-qualified.** os/arch/distro seed the solve `OptionMap` (same source spk uses: `HOST_OPTIONS`, `option_map/mod.rs:74-93`) and join the **cache key, lock key, digest identity inputs, and invocation annotations**. Same path on rhel8 and rocky9 = two resolutions, two digests. This is what makes the ABI mismatch keyable instead of hidden.

**Override: host-agnostic.**
- Per resolution: `--no-host` clears host options (`OptionMap::default()`, grounds on spk's `flags.rs:335-340`). Host-independent digest, reproduces on any node.
- Per layer/mapping: `agnostic: true` excludes the entry from the host-qualified partition, so it dedupes across platforms and shares one render.

**The digest is computed over two partitions** — host-qualified and agnostic. Reproduction on a different host reuses the agnostic partition and re-resolves (or refuses, per policy) the qualified one.

**Solver determinism (C3):** pin one solver implementation explicitly (call `SolverMut::solve()`, not `run_multi_solve` which returns whichever task finishes first, `io.rs:1394-1485`), record which solver in invocation annotations, and **canonicalize solved-layer order** (sort by package name then component) before assembly — `Solution` order is "arbitrary" per `exec.rs:88-96`.

---

## 5. Stack assembly and env composition

### Band order (bottom to top), settling C7/D-D

```
1. agnostic base + config layers      (slot 1, agnostic — shared across hosts AND shows)
2. host-qualified base layers         (slot 1, qualified)
3. solved spk layers                  (Rule 2 output, canonically ordered)
4. tag + discovered layers            (slots 2/3, depth-interleaved)
5. mapping layers                     (compiled, same depth order)
6. mask layers                        (must sit above what they mask)
7. personal override layers           (slot 4)
```

Agnostic base/config at the very bottom is deliberate: it's what dedupes across hosts and shows, recovering the flatten-group sharing that host-qualification would otherwise fragment (C7). **Canonicalization pass required before building:** first-occurrence-wins, drop later duplicate digests — because `Stack::push` otherwise *promotes* a re-referenced layer to the top (`graph/stack.rs:79-107`), and nested `.unique()` keeps the *bottom* copy (`resolve.rs:466-469`); opposite semantics, so the resolver must dedupe deterministically itself (D1).

**Open call (C7 residue):** spk-vs-promoted precedence when both touch the same path — one explicit sentence needed. Default proposal: promoted (band 5) above solved spk (band 3), so an explicit file placement wins over a solved package's file, matching "explicit overrides win."

### Env composition (C2 / D-B) — resolver-owned, three sources

There are **three** sources of env ops, not two, and the resolver must own all of them because `startup.d` mechanically wins otherwise:

1. hierarchy `env:` ops (spec files, per level);
2. **solved packages' runtime env ops**, read from their specs via the `spk_schema` runtime-environment API (not from the generated `startup.d` scripts);
3. self-describing layer env ops (annotations on promoted layers).

The resolver composes all three in band order into final values, applies them via the enter path, and **masks `/spfs/etc/spfs/startup.d`** with a mask layer so package scripts never double-apply (`spk-build/.../binary.rs:688-732` bakes them; `startup_sh.rs:29-46` sources them after the resolver's own exports).

**Mechanism gap to close (C2c):** `--environment-override` is only reachable through the private `build_spfs_enter_command` and only for `variable_names_to_preserve` (`bootstrap.rs:345-372`). The resolver either reimplements `which_spfs("enter")` + arg construction, or this is opened upstream. Note `EnvOp` has **five** variants including `Priority` (which only affects startup.d ordering) and `Comment` — the resolver models `Set`/`Prepend`/`Append` and ignores the startup.d-specific ones (`spk-schema/src/environ.rs:75-84`). Also reconcile against `solution.to_environment()`'s `SPK_PKG_*` injection (C2b) — those are set by spk's own machinery; decide order explicitly.

---

## 6. Config store (C1) — resolver-owned read path

`config:` values ride as annotations, but **the resolver never reads them via `all_annotations()` or `annotation()`** — both traverse breadth-first, so a nested platform's annotation wins regardless of stack position, inverting precedence (C1/S1), and the flat readers are additionally lowest-wins vs highest-wins and mutually inconsistent (verified, `runtime/storage.rs:571-593`, `:1073-1159`; the flat lowest-wins behavior is even an *intentional, tested* upstream invariant, `storage_test.rs:110-124`).

**The blessed read path:**

1. At resolve time, compose all `config:` sections in band order into **one merged blob** (canonical JSON) written as a single annotation (`ilm.cfg`) on a synthetic layer at the top of the resolved stack. Per-source values go in a second annotation (`ilm.cfg.src`) for `--explain`/`why`.
2. Readers (`spfs env config get`, the Python API) load `ilm.cfg` with a **custom top-down stack walk** — walk `status.stack` top to bottom, first hit wins, stop early. This is O(few) reads, not the O(graph) bottom-up walk (D3 cost).
3. **Distinguish absent from unreadable** — the built-in traversals silently swallow read errors and return "absent," so a missing/corrupt layer becomes a wrong `default` (D3). The blessed reader surfaces storage faults as errors.

**Writes (`config set`) are runtime-scoped and ephemeral**, and honest about cost: each `set` rewrites the runtime's data tag stream (`storage/fs/tag.rs:305-357`), so **batch** (`add_annotations`, one layer) and never loop `add_annotation` (one layer per key). No per-frame checkpointing (D2). Empty batch is a no-op, not a write (an empty annotation layer hard-errors on digest, D config §3). Duplicate keys within a batch dedupe last-wins before building. Unset uses a printable tombstone the blessed reader treats as absent (not `\x00`, which prints raw, D5); tombstones are documented to leak into any `spfs commit platform` of that runtime (D5).

**Rules:** values are UTF-8 strings (structured = embedded YAML); ≤16KiB inline, larger spills to a blob (`graph/annotation.rs:17`); flat namespace, prefix by owner (`show.*`, `app.<dcc>.*`, `spk.*` reserved); **functional config is in the digest, invocation data is not** (annotations are in the layer digest regardless of encoding, `layer.rs:122-145` — so timestamps/hosts go on the runtime post-digest, not the platform). Requires FlatBuffers encoding (Legacy blocks annotations, `layer.rs:147-153`; also guard the silent `HeaderBuilder` Legacy fallback on config-load error).

---

## 7. Resolution algorithm

```
 1. canonicalize target path (cwd or arg); classify against allowlist (§8)
 2. build context: providers → validated vars (§3); conflict policy; --pure honored
 3. derive candidate tags from context; validate each component-wise post-substitution
 4. probe all candidates in parallel; classify ENOENT=miss vs hard-fail (§3)      [slot 2]
 5. crawl filesystem upward for .spfs-env.yml to boundary/allowlist edge           [slot 3]
 6. load base (system config) [slot 1]; load personal ($HOME/--with/$SPFS_ENV_WITH) [slot 4]
 7. seed host options (or empty if --no-host); compute input fingerprint incl. host block
 8. cache lookup on (files+hashes + resolved tag targets + overrides + host block + solver)
      hit → platform digest → step 15
 9. accumulate: layers/mappings/mask ordered (Rule 1); requests−replaces → one set (Rule 2);
      env ops from all three sources (Rule 3); config sections
10. compile mappings → metadata layers (re-rooted blob digests, /spfs stripped, explicit modes);
      compile mask → mask layer
11. ONE spk solve on the request set, pinned solver, host-qualified options → layer digests;
      canonicalize solved-layer order
12. assemble stack in band order (§5); dedupe first-occurrence-wins; partition qualified/agnostic
13. compose env → final values; compose config → merged ilm.cfg blob
14. compute platform digest locally; write_object to the CANONICAL repo and PUSH to origin
      (workers sync from origin — a local-only platform is UnknownObject on every worker)
15. create runtime (ephemeral by default); set stack; add invocation annotations (host, sources,
      hashes, solver, live-layer flag) to the RUNTIME, not the platform
16. mask startup.d; apply composed env overrides; write client-side journal entry; exec
```

**No auto-tag** (D-D). v1's step 14 push_tag is gone. Tagging happens only via `spfs env commit`, farm submit, or CI publish.

**Object placement (findings gap):** step 14 must push resolver-created objects (platform, mapping/mask layers, `ilm.cfg` layer) to the repo workers read (origin), and the design must name which repo is canonical. A platform written only locally fails on every farm worker.

---

## 8. Security model (S1-S4)

- **Resolver builds its own `spfs::Config`** from a fixed root-owned file with the environment sources removed, for at least `remote.*`, `storage.root`, `storage.tag_namespace`. spfs's `load_config()`/`get_config()` is **not** on the trusted path — `SPFS_REMOTE_ORIGIN_ADDRESS`/`SPFS_STORAGE_*` otherwise re-point the trusted repo from an unprivileged env var (S3, `config.rs:188-205,628-632,904-921`). Note `Tag::new` calls `get_config()` internally, so this entanglement is real.
- **Context values validated component-wise** (§3) — closes the `..` read-path namespace escape (S1, a fully valid `TagSpec` today with no normalization anywhere; `to_logical_path` is never called) and the empty-value wrong-level hit (S2).
- **Slot-3 trust (S4):** decide explicitly — `/show/**` is routinely group-writable and artist-owned, so the v1 "owned by root or trusted gid, not group-writable" makes slot 3 dead on arrival. Proposed: allowlist-by-path (config) + owned-by-a-show-account (not world-writable), TOCTOU-safe (open, then `fstat` the fd, read from the fd — not stat-then-open). This is weaker than `safe.directory` and the doc states that tradeoff.
- **Canonicalize before allowlist match** (automount/symlink divergence, `runtime/storage.rs:1350`), but handle: deleted cwd, and the fact that canonicalize itself triggers automount.
- **Crawl needs its own loop/dedup** (inode-set, not path-set) — `SEEN_SPEC_FILES` doesn't apply (wrong suffix) and is a never-cleared global that would break a resolver daemon / the differ / shadow mode (B7). Dedupe `$HOME`-under-crawled-root so its file isn't applied twice (B8).

---

## 9. spk integration (D-A, C4)

spk packages are a **separate, first-class mechanism** — not merged with promoted layers (D-A). The resolver:

- Calls `spk_solve::Solver` directly (`SolverImpl::Step` or `ResolvoSolver`, pinned), seeds options from host (§4), adds repos and the accumulated request set with `replaces` filtered out, `set_binary_only(true)` unless build-from-source is handled, `solve() → Solution`, then `spk_exec::solution_to_resolved_runtime_layers → layers()` (all public, `exec.rs:88,183,248`). Splices digests into band 3.
- **Handles `BuildFromSource`** — `resolve_runtime_layers` hard-errors on it (`exec.rs:191-201`) and the build helper lives in `spk-cli-common`; decide: depend on it, reimplement, or `set_binary_only` + clear error (B10).
- Stamps an **`ilm.env.owner` marker annotation** so an upstream ~5-line guard in `setup_runtime` refuses to replace a marked stack without `--force` (C4). `spk install`/`spk env --no-runtime` otherwise silently wipe the hierarchy stack + config + provenance mid-session (`cmd_install.rs:125`, `exec.rs:352`). Document this; the guard is a small upstream change (confirm appetite).
- Reads solved packages' env ops for composition (§5), does **not** rely on their `startup.d` scripts.
- Promoted layers are labeled unmanaged (`ilm.layer.managed = false`) and host-fingerprinted (§4); `why --package` never attributes them to a solve.

**Solve latency:** an spk solve is not 200ms (spk has automatic mid-solve verbosity escalation and disabled-by-default timeouts, `config.rs:57-68`), so the **cache is a prerequisite** for `cd`-triggered resolution, not an optimization. The explanation/determinism tradeoff: `solve()` gives deterministic order but a bare error; the rich Rule-2 explanation comes from `DecisionFormatter` which owns stdout — the resolver either reimplements the formatter or captures it into `--explain`.

---

## 10. Retention, ephemerality, and commit (D-D)

| Class | Created by | Tag namespace | Retention |
| --- | --- | --- | --- |
| Ephemeral | plain `spfs env` | none | best-effort, ~15-min GC floor (`cmd_clean.rs:184`) |
| Committed | `spfs env commit` | `committed/<user>/...` | until deleted |
| Farm job | submit | `jobs` | prune schedule |
| Service | service resolve | `services` | prune schedule |
| Published | CI | `ilm/**` | permanent |

**Ephemeral is loud:** banner at enter, retention class first in `status`. **`spfs env commit`** captures stack + committed edits + resolved host block + provenance, tags in a durable namespace, and is the **sole reproduce-by-digest guarantee**. `--digest <D>` on a collected ephemeral digest reports "ephemeral from <when>, not committed, aged out — nearest committed: <tag>" (client journal supplies the metadata; the object is gone). Separate **tag namespaces** are load-bearing: prune is repo-wide within a namespace and the clean flags are a mutually-exclusive clap group (C9/D10), so per-class retention *requires* namespace separation.

---

## 11. Command surface

```
spfs env [PATH]                     resolve + enter (ephemeral)
spfs env --app maya [PATH] -- maya  app profile merged into the solve
spfs env --with REF [PATH]          slot-4 override
spfs env --no-host [PATH]           host-agnostic resolution
spfs env --pure [PATH]              flags + path only, ignore env providers
spfs env --as-of @DATE [PATH]       tags only, pinned remote, no slots 3/4, no publish
spfs env --digest DIGEST            reproduce a committed environment
spfs env commit [--tag ...]         ephemeral → persistent; the reproduce guarantee
spfs env promote SRC --dest ... --tag ...   author an UNMANAGED layer (files you vouch for)
spfs env status                     retention class, context block, sources, warnings
spfs env why <path>|--var V|--package P   provenance per composition rule
spfs env explain                    full resolution trace
spfs env diff [--mine|--lock|--as-of]     three-view diff (layers/files/env+config)
spfs env doctor                     repo reach, allowlist, encoding, host block, cache, flatten budget
```

Ships as `spfs-env` on PATH via external-subcommand dispatch — **zero spfs changes** for dispatch (`bin.rs:98`, `which_spfs`); have the Python backend exec `spfs-env` directly (a missing binary otherwise exits 1 indistinguishably). Cannot be named `spfs enter` (that dispatches to the privileged `spfs-enter`). **Mandate absolute paths for `spfs-enter`/`spfs-render`/`spfs-monitor`** so a `/spfs` layer shipping those binaries can't shadow them via PATH (nested-runtime hazard, farm agent).

`--as-of` must resolve against an **explicitly pinned remote** (the local repo can't be pinned — `when` exists only on `RemoteConfig`, `config.rs:213-223`; local-then-remote read order otherwise returns today's answer for locally-present tags, B4), imply `--no-publish` (pinned repos hard-error on push), and validate the time string before `TimeSpec::parse` (which requires a leading `@`/`~` and panics on empty/multibyte-first-char, B5). Note the upstream `@HH:MM` = UTC-not-local bug (B5) and the un-reparseable pinned `address()` (B6).

---

## 12. Build order

Steps 1-3 prove ergonomics on a real show path before committing to the rest.

1. **Path/context → tag mapping + validation + probe.** Component-wise post-substitution validation, error classification, alias map. Table-driven tests: `cd /`, no spec anywhere, `$HOME` under root, symlink loop, deleted cwd, `..`/empty/`~N`/space/non-ASCII candidates. No filesystem mutation.
2. **Env-proofed Config + upward crawl** with boundaries, allowlist, ownership, inode-dedup. Security tests first (S1-S3).
3. **Rule 1 accumulation + band-ordered stack assembly + canonical dedup.** Emit a runtime, enter, confirm mount.
4. **Mapping/mask layer compilation** — re-rooted blob digests, `/spfs` strip, explicit modes, file-granularity, symlink detect/reject, FUSE size/mode fidelity. Verify identical mappings → identical digests.
5. **Config store: resolver-owned top-down read path** (the C1 keystone) + write/batch/tombstone + the duplicate-key/nested-platform tests upstream never wrote.
6. **Rule 3 composition** across all three sources + startup.d masking + the enter-override mechanism (upstream or reimplemented).
7. **Rule 2 + spk solve** (pinned solver, host options, `replaces`, build-from-source policy, owner-marker guard).
8. **Host qualification + identity** (partitioned digest, cache/lock keys, `--no-host`/`agnostic`).
9. **Retention + `commit` + ephemeral UX + client journal.**
10. **`explain`/`why`/`diff`/`doctor`; differ; shadow mode.**

## 13. Upstream changes this needs (small, itemized)

- Open `build_spfs_enter_command` (or an override-injection API) so the resolver can apply computed env overrides (C2c).
- `ilm.env.owner`-style stack-ownership guard in `setup_runtime` (C4).
- Bug fixes surfaced (independent of the resolver): `all_annotations()`/`annotation()` nested-platform-BFS test + doc warning; `TimeSpec` `@HH:MM` UTC bug and empty/multibyte panic (B5); `PinnedRepository::address()` un-reparseable `when=` (B6); tag-validation char-index panic (B1); tag.rs doc comments claiming `:` legal / versions negative.

The whole resolver remains a **tooling layer** over spfs + spk-solve (the same relationship spk-exec already has), shippable without forking; the upstream items above are small and additive, and only the enter-override and the ownership guard block a clean build.
