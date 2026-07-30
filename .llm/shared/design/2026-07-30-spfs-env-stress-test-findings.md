---
date: 2026-07-30T01:30:00-0700
repository: spk
git_commit: 67bfc091
branch: worktree-spfs-discovery-doc
title: "spfs env design: adversarial stress-test findings"
status: complete
tags: [design, spfs, ilm, environment, review, findings]
reviews:
  - 2026-07-29-ilm-env-hierarchy-spec.md
  - 2026-07-29-spfs-env-user-guide.md
  - 2026-07-30-spfs-env-python-api.md
method: "7 parallel adversarial agents, each walking one persona's workflow end-to-end and verifying every load-bearing claim against crates/spfs and crates/spk* at this commit"
---

# Stress-test findings: the `spfs env` design

Seven adversarial agents each took one persona (comp artist, pipeline TD ×2, farm/service engineer, DCC integrator, config/annotation API, security/resolution) and walked a real workflow end to end, verifying every load-bearing claim against the actual code. Their job was to break the design, not confirm it.

**Verdict: the core model is sound; the design as written is not buildable.** "An environment is a digest," four slots, and three composition rules survive scrutiny. But roughly a dozen load-bearing mechanisms were shown WRONG against the code, two of them security holes, and the two flagship workflows (the TD library release in guide §5, the farm submit in §8) both fail as written. Every finding below is code-verified; file:line citations are the agents' and were spot-checked.

This document supersedes specific claims in the three design docs. Those docs now carry a banner pointing here; where a claim is contradicted below, **this document wins.**

## How to read this

Findings are grouped by **convergence** — how many independent agents hit the same wall — because convergence is the strongest signal of a real architectural problem versus a persona-specific nitpick. Within each tier, ranked by blast radius.

Severity: **BLOCKER** (design cannot be built as written), **SECURITY**, **FORCED DECISION** (a "decision" the doc deferred is actually already constrained by the code), **BUG** (doc contradicts code), **GAP** (underspecified in a way that changes the design).

---

## Tier 1 — Converged across 4+ agents (the real architecture problems)

### C1. The annotation read path is inverted, and it's BFS not stack-order. `[artist, TD×2, farm, DCC — 5 agents]`

**BLOCKER.** The entire config-precedence model rests on "read with `all_annotations()`, highest layer wins." The code says otherwise on two counts:

1. `all_annotations()` returns `BTreeMap<String,String>` — **one value per key** (`runtime/storage.rs:581-593`). Two layers writing the same key: the map collapses them, lower silently lost. Per-layer-unique keys: iteration is key order, not stack order. Either way Rule 3 composition is unbuildable on this API.
2. Traversal is **breadth-first**, not stack order. `find_annotation_key_value_pairs` pushes a nested platform's children onto a *later* round, so annotations inside a nested platform are always collected last and win the BTreeMap insert **regardless of stack position** (`runtime/storage.rs`, the `digests_to_process`/`next_iter_digests` loop). Guide §3.3 *mandates* nested platforms at the bottom of every stack — so the recommended shape makes the bottom win, the exact inversion of the intended rule. `annotation()` (which spk uses for `spk_solve`, `spk-cli/common/src/env.rs:94`) breaks symmetrically: first-hit-in-BFS is shallowest, not lowest.

**Consequence:** the design's "merged `ilm.cfg` blob on a synthetic top layer" mitigation survives *only* because nothing else writes that one key. Every other annotation read (invocation data, self-describing env ops, `spk_solve`) goes through the broken path.

**Fix (forced):** the resolver must walk `status.stack` itself in explicit order and call `find_annotation_key_value_pairs` per digest, composing in the order it controls. Strike every `all_annotations()`/`annotation()` recommendation from all three docs. The Python API's "blessed read path" is now *mandatory infrastructure*, not a convenience — and it must not delegate to either built-in reader.

### C2. `startup.d` mechanically beats the resolver's env composition. `[TD×2, DCC — 3 agents, deepest trace]`

**BLOCKER.** The spec is emphatic that env composition happens in the resolver and applies via `--environment-override`, "never via startup.d." The code makes that impossible:

- `--environment-override` is not applied to the process; it is written *into* the generated startup script, emitting `export KEY=VALUE` **first**, then sourcing `/spfs/etc/spfs/startup.d/*.sh` (`runtime/startup_sh.rs:29-46`).
- Every spk package with runtime env ops ships its own `/spfs/etc/spfs/startup.d/spk_<name>.sh`, baked at build time (`spk-build/src/build/binary.rs:688-732`).

So the resolver's composed PATH/PYTHONPATH is the **seed**, and every solved package prepends on top of it afterward, in alphabetical file order. The resolver's ops are *lowest* precedence, not highest. `spfs env why --var PYTHONPATH` would be wrong for any environment containing an spk package with env ops — i.e. nearly all of them.

Corollary: `EnvOp` has **five** variants, not the three the spec names — `Append`, `Comment`, `Prepend`, `Priority`, `Set` (`spk-schema/src/environ.rs:75-84`) — and `Priority` exists *solely* to rename the startup.d file so it sorts differently. "Reuse spk's schema shape" imports a variant meaningless in the resolver's model and omits the one knob packages use for ordering.

**Also (C2b):** `solution.to_environment()` is a *third* env-var mechanism — it injects `SPK_ACTIVE_PREFIX`, `SPK_PKG_<name>_*` for every package and scrubs inherited `SPK_PKG_*` (`spk-solve/.../solution.rs:388`). Nowhere does the design order package-supplied vars against hierarchy ops. This is the real content of the deferred "Decision 5."

**Also (C2c):** the mechanism the spec names to apply overrides is not even reachable. `build_command_for_runtime` delegates to the **private** `build_spfs_enter_command`, which emits `--environment-override` *only* for `config.environment.variable_names_to_preserve` (`bootstrap.rs:131-142,345-372`). There is no public path to inject computed overrides — the resolver must reimplement `which_spfs("enter")` + arg construction itself, or take an upstream change. `[config-API, farm — confirmed twice]`

**Fix (forced, likely upstream):** the resolver reads solved packages' env ops from their specs (`spk_schema` runtime environment API), composes them into its ordered result at the solved-layers stack position, then **masks `/spfs/etc/spfs/startup.d`** with a mask layer so package scripts never fire. This cannot be fixed inside the resolver alone without that mask, and the interaction with `to_environment()` needs an explicit rule. Either that, or the design stops claiming it owns env composition.

### C3. "An environment is a digest / same inputs, same digest" is false. `[TD×2, farm, DCC — 4 agents]`

**BLOCKER + FORCED DECISION.** The platform digest is a pure function of the stack, but the *stack* is not a pure function of the inputs the design records. A solve additionally depends on:

- **the resolving host's OS/arch/distro** — solve options default to `HOST_OPTIONS`, auto-detected from `/etc/os-release`, and builds are filtered against them (`spk-cli/common/src/flags.rs:333-341`, `option_map/mod.rs:71-98`, `spk-storage/.../repository.rs:258`). None of it is in the cache key, digest identity, or `--explain`.
- **live spk repo state** — a package published five minutes ago changes the answer with no tag in the key moving.
- **solver implementation** — `StepSolver` vs `ResolvoSolver` return solutions in different order (`spk-solve/src/solver.rs`), and `--solver-to-run all` returns whichever *finishes first* (`io.rs:1394-1485`). Different order → different `Stack` → **different platform digest for an identical solve.**
- **`Solution` layer order is documented in-tree as "arbitrary"** (`spk-exec/src/exec.rs:88-96`), and it's one layer **per component**, not per package.

**Consequence, and it is the ABI bug from the TD scenario:** "resolve at submit, run by digest" actively *defeats* spk's existing host-option protection — the submitter's distro selects the builds, then 5,000 heterogeneous workers mount them, looking perfectly reproducible while the C extension segfaults.

**Fix (forced):** fold host options + solver impl (+ ideally a per-resolution pinned repo) into the cache key and invocation annotations; canonicalize solved-layer order (sort by package/component) before assembly; pin one solver explicitly. Restate the digest-stability claim honestly.

### C4. spk replaces the runtime stack; nothing guards the hierarchy. `[TD, farm, DCC, spk — 4 agents]`

**BLOCKER, partially known.** The design flags that `setup_runtime` does `rt.status.stack = Stack::from_iter(...)` (confirmed verbatim, `spk-exec/src/exec.rs:352`) and picks "resolver owns the solve." But the second-order effects are unaddressed:

- The attribution is **wrong for the default path**: plain `spk env` re-execs into a *new empty* runtime (`spk-cli/common/src/flags.rs:191`, ref = `ENV_SPEC_EMPTY`), losing the hierarchy by whole-runtime replacement. The *in-place* destruction happens on `spk env --no-runtime` and, unmentioned, **`spk install`** (`cmd-install/src/cmd_install.rs:125`) — which silently wipes the hierarchy stack, config annotation, and provenance mid-session with no new shell to signal it.
- A promoted (raw-layer) library has no `/spfs/spk/pkg` metadata (`spk-storage/.../runtime.rs:35`), so `spk` inside the runtime can't see it and *will drop it* from the replaced stack.

**Fix:** stamp an `ilm.env.owner` marker annotation; add a ~5-line upstream guard in `setup_runtime` refusing to replace a marked stack without `--force`. Document `spk install` explicitly.

---

## Tier 2 — Converged across 2-3 agents

### C5. Untagged resolutions are garbage-collected after 15 minutes. `[farm, spk — 2 agents, highest single blast radius]`

**BLOCKER.** The design's cornerstone scale decision is "do not auto-tag every resolution; record digests in locks." But `Cleaner` roots GC on **tags only** (`clean.rs:337-345`), and the CLI hardcodes `with_required_age(Duration::minutes(15))` (`cmd-clean/src/cmd_clean.rs:184`). Runtimes and lock files are not roots. So any untagged resolved platform — and its compiled mapping/mask layers and merged-config annotation — is reaped 15 minutes after creation by a routine clean. This breaks, on the design's own terms:

- `spfs env --digest <D>` reproduce-from-lock → `UnknownObject`.
- `spfs diff <old> <new>` lock alerts → old platform gone.
- the resolution cache → caches a collected digest.
- the "nearest survivor (diff attached)" promise → the record it needs was pruned in the same pass.

The design cites `Cleaner`'s memoization as evidence GC is cheap — it read the mark *cost* but not the mark *roots*. **The design cannot simultaneously refuse to tag and promise digest reproduction.**

**Fix (forced decision):** either tag every resolution (accepting the tag-count cost the design argued against — and put `jobs/`, `services/`, interactive locks in their own tag namespaces so per-prefix retention and repo-wide pruning don't collide, see C9), or add a non-tag GC root upstream, or downgrade locks to change-detectors with an explicit TTL and drop the reproduce-by-digest promise for untagged resolutions.

### C6. Mapping layers are not metadata-only, and the render cost contradicts the design. `[spk, TD — 2 agents]`

**BLOCKER + internal contradiction.** Two problems with the novel "zero-copy mapping layer":

1. `src` is a **host path**; spfs has no host-path → committed-blob-digest index. So you either `commit_dir` (reads every byte — not metadata-only) or require `src` to be inside the already-resolved `/spfs` (circular; and the only lookup, `find_path`, needs an *active runtime*, `find_path.rs:44-51`). The "no file data moves" claim is unimplementable as framed.
2. Even accepting forged manifests (which *do* work — `Entry` fields are `pub`, `mknod`/`create_layer_from_manifest` public, no blob-existence check), renders are keyed on **manifest digest** (`storage/fs/manifest_render_path.rs:12`). A mapping layer is a new manifest → a new render dir → **one hardlink inode per remapped file, per user**. A whole-tree mapping (`/sww/gfx/maya/2024`) is O(10⁵) inodes per user. This is *exactly* the "sleeper problem" the design flags to reject per-layer filters (spec §Scale) — then makes mapping layers "the default." Sharpest internal contradiction in the docs.

**Fix:** restrict mappings to file-granularity (`dest: /spfs/bin/mytool`), forbid/gate whole-tree mappings, correct the cost table to "metadata + one render + N inodes/user." Prefer mask layers (genuinely cheap, C-verified) wherever the goal is removal not relocation.

**Plus three mapping-layer bugs** (`spk` agent): the recipe omits `mkdirs` so `mknod` errors `NotFound` on missing parents; synthesized parents are mode **0o40777** (world-writable `/spfs/maya`); and **every `dest: /spfs/...` example lands at `/spfs/spfs/...`** because manifest paths are `/spfs`-relative and nothing strips the prefix (`tracking/manifest.rs:259-271` strips only leading `/` and `./`, not `spfs/`). Symlink entries silently dangle on relocation; forged `size`/`mode` work on overlayfs but truncate on FUSE (`spfs-vfs/src/fuse.rs:176-179`).

### C7. Decision 4 is mutually exclusive with the root-first flatten-sharing rationale. `[spk agent, decisive]`

**FORCED DECISION.** Root-first ordering is justified by "common ancestors sit at the bottom, so shots on one show share flatten groups." But flattening consumes **from the bottom** (`resolve.rs:313`), and Decision 4 puts *solved spk layers* at stack position 2 — and the solve is exactly what varies per shot. So the bottom 7/14/21 layers are solve output, and two shots differing by one `requests:` entry get **different flatten groups and zero render sharing** — the precise outcome the design says leaf-first would cause. The doc argues for both in adjacent sections. **Pick one:** spk layers above the shared base but below per-level content, or drop the sharing claim.

### C8. The flagship dev workflow (guide §5) routes around the safety mechanism. `[TD×2 — 2 agents]`

**BLOCKER for the advertised workflow.** `spfs env promote` produces a **raw spfs layer with no package metadata**. So a released library's dependencies (`numpy>=1.24`) never enter the request set: the solve succeeds against the show's numpy 1.21, and the C extension ImportErrors/segfaults at 11pm in a farm job. The design's own advice ("prefer `requests` over `layers` wherever a package exists") is correct and the flagship workflow ignores it, because promote never creates a package. And `promote` has no build step, so the gcc/python ABI the `.so` was compiled against is recorded nowhere — nothing stops a rhel8 `.so` mounting on rocky9.

Compounding: guide §5.2's promote uses the **unversioned** dest `/spfs/ilm/lib/python`, directly violating §6.1's own versioned-prefix mandate — a self-inflicted silent mixed install on any partial-overlap upgrade.

**Fix:** make `promote` produce an spk package (or add first-class `requires:` to layer metadata + a recorded host-option fingerprint). This single decision collapses C8, the rollback gap (C10), and the "no build-env description" gap out of existence.

### C9. `spfs clean` prune is repo-wide and the doc's flags don't exist. `[farm agent]`

**BUG + design gap.** Two things:

- The §3.5 retention recipe uses flag names that are the *internal builder* methods, not the CLI: real flags are `--prune-if-older-than`/`--keep-if-newer-than`/`--prune-if-more-than` etc. (`cmd_clean.rs:58-80`), and `m` in age strings means **minutes, not months** (`age_to_date`).
- There is no tag-path filter; prune applies to *every* stream in the active namespace (`clean.rs:441-483`). The §3.5 command would prune `ilm/show/*` rollback history and delete aged immutable tags (`ilm/app/maya/2024.2`) outright. The only scoping is tag namespaces, which the naming scheme doesn't use.

**Fix:** put `jobs/`, `services/`, and interactive-lock tags in dedicated tag namespaces; rewrite §3.5 with real flags and per-namespace retention; state that weekly cadence means deletion lands at retention+0..7 days.

---

## Tier 3 — Security (security/resolution agent, code-verified)

### S1. `..` in a context var escapes the repository on the READ path. `[SECURITY — critical]`

`split_tag_spec("ilm/show/../../user/mallory")` produces a **fully valid TagSpec** (org `ilm/show/../../user`, name `mallory`): `rsplitn(2,'/')` splits on the last slash only, and `_find_org_error` permits `.` and `/` (`tracking/tag.rs:270,352-364`). Nothing normalizes — `relative-path`'s `to_path` writes `..` components verbatim and spfs never calls `to_logical_path` (zero hits in the tree). So on the **read** path (`storage/fs/tag.rs:628`, direct `File::open`) enough `../` reads `/home/mallory/fake.tag`, whose target digest becomes a slot-1/slot-2 layer. **This is a complete bypass of "slots 1 and 2 require repository write access," with no repo access at all.** (Write is accidentally blocked by a ParentDir check in `makedirs`; delete is *not* — arbitrary `*.tag` removal.)

The design's stated mitigation ("no dots unless a var opts in") is insufficient: `..` is dots-only, and your own naming scheme wants dotted values (`release = 2026.06`). **Required rule set:** reject `/` in any substituted value unconditionally; reject any value whose components equal `.`/`..`; reject empty values (S2); and **re-validate the fully assembled candidate component-by-component after substitution**. Do not treat `split_tag_spec` as a backstop — it isn't one.

### S2. Empty context var hits the wrong level's tag. `[SECURITY — high]`

`SHOW=""` in candidate `ilm/show/{show}/{seq}` → `ilm/show//tst0100`, and empty path components are silently dropped, so this resolves to the tag of a show *named* `tst0100` — a hit on the wrong level, not a miss. Set-but-empty env vars are extremely common in shell wrappers, and the design never says whether set-but-empty means "unset/skip" or "value." Same aliasing (`//`, leading `/`, `~N`) makes the path→tag map **non-injective**, which also corrupts the cache key and produces multiple Tag digests for one stream.

### S3. `SPFS_*` env vars re-point the repo the resolver trusts. `[SECURITY — high]`

The design correctly notes `load_config` lets `SPFS_*` override `/etc`, then draws the boundary in the wrong place. Everything slots 1/2 depend on transits spfs `Config`: `SPFS_REMOTE_ORIGIN_ADDRESS=file:/tmp/evil` re-points `origin` (`config.rs:188-205,628-632`), `SPFS_STORAGE_ROOT`/`SPFS_STORAGE_TAG_NAMESPACE` re-point the local repo and every tag read. So "slots 1/2 require governed repo access" evaporates from an *unprivileged env var*. **The resolver must build its own `spfs::Config` from a fixed root-owned file with the environment sources removed** for at least `remote.*`, `storage.root`, `storage.tag_namespace` — `load_config()`/`get_config()` cannot be on the trusted path. (Note `Tag::new` calls `get_config()` internally for the user field, so this entangles more than it looks.)

### S4. Slot-3 ownership check as written makes slot 3 unusable. `[design-level]`

"Owned by root or a trusted gid, not group/world writable" versus the reality that `/show/**` is routinely group-writable by the show group and files are owned by artists. Either the check is relaxed (slot 3 becomes trust-by-path, which `safe.directory` explicitly refused) or slot 3 is dead on arrival. Decide in the doc. And the check must be TOCTOU-safe: open, `fstat` the fd, read from the fd — not stat-then-open.

---

## Tier 4 — Correctness bugs and forced decisions (single-agent, code-verified)

### B1. The "validate and error" path PANICS on non-ASCII. `[security agent]`
`tag.rs:294-320` uses a **char** index from `_find_name_error` to **byte**-slice the string. A sequence dir `séquence` → panic "byte index is not a char boundary." The exact error path the design relies on crashes. Resolver must validate paths itself.

### B2. `~` in a directory name silently misparses. `[security agent]`
`tag.rs:279` splits on `~` *before* any charset check, so `tst0100~2` → tag `tst0100`, version 2 (two versions back). Not a charset violation — an accepted parse with different meaning. Backup/scratch dirs (`shot~1`, `~old`) hit this. The path→tag map needs a canonical-form check (does the derived tag re-serialize to the exact input?), not a charset check.

### B3. Probe misses are only cheap for `ENOENT`. `[security agent]`
`is_os_not_found()` matches only `ENOENT` (`error.rs:325-338`), so `ENAMETOOLONG` (long level names, ~246 char leaf limit) and `EACCES` (per-show tag ACLs) turn a parallel probe into a **hard failure**, not a skipped gap. "Gaps are legal" needs the resolver to explicitly classify probe errors into miss vs fail.

### B4. `--as-of` cannot pin the LOCAL repo. `[security agent]`
`when`/`into_pinned` exist only on `RemoteConfig` (`config.rs:213-223,365-368`); the local repo (`get_opened_local_repository`) has no pin. spfs reads local-then-remote, so after one prior resolution the locally-present tags resolve **unpinned** — `--as-of` silently returns a per-tag mixture of today and the pin date, the exact incoherence the design forbids for slots 3/4, now *inside* slot 2. `--as-of` must resolve against an explicitly pinned remote handle only, and `--as-of` must imply `--no-publish` (a pinned repo hard-errors on `push_tag`).

### B5. `--as-of @18:00` pins 18:00 UTC, not local. `[farm agent]`
`TimeSpec::parse_absolute_time` time-only branch stamps today's local date but forces UTC (`time_spec.rs:131-158`); the date-only branch correctly uses `Local`. At a PDT facility `@18:00` pins 11am and silently drops everything published after. Upstream bug; `spfs env` should echo the resolved pin as full RFC3339 local. (Also: `TimeSpec::parse` `split_at(1)` panics on empty/multibyte-first-char input — guard `--as-of`.)

### B6. `PinnedRepository::address()` emits an un-reparseable `when=`. `[security agent]`
It writes `DateTime` `Display` (`2026-07-01 00:00:00 UTC`, no `@`) instead of `TimeSpec` `Display` (`storage/pinned/repository.rs:163-167`), so re-opening a pinned handle's address fails `TimeSpec::parse`. Breaks passing pinned remotes to subprocesses / farm payloads / pasteable `--explain`. One-line fix upstream; spk already does this right.

### B7. `SEEN_SPEC_FILES` protects nothing the resolver does. `[security agent]`
The guard only fires for files ending `.spfs.yaml` reaching `SpecFile::parse` — but the design's files are `.spfs-env.yml` (wrong suffix, and `.yml`≠`.yaml`), so they never reach it. The resolver needs its own loop/dedup detection (inode-set, not path-set — the guard doesn't canonicalize either). And `SEEN_SPEC_FILES` is a never-cleared process global (`clear_seen_spec_file_cache` called nowhere), so a resolver **daemon**, the **differ**, and **shadow mode** all break if they route through spfs's parser — they must not.

### B8. `$HOME` under a crawled root is read twice. `[security agent]`
When `$HOME` is inside the allowlist (`/show/abc/users/apaxson`), step 4 (slot 3 crawl) and step 6 (slot 4 home) both pick up `$HOME/.spfs-env.yml`; Rule 3 ops apply twice at two stack positions. Dedup and slot-precedence unspecified.

### B9. Bind mounts and flattening mean the runtime stack ≠ the digested platform. `[artist, TD]`
A bind mount commits a skeleton mount-point layer onto the stack (`runtime/storage.rs:613-620`); flattening folds a `HashSet` of manifests via `to_platform()`. So `spfs env status`'s single `platform` line won't match `spfs runtime info` for any bind-mounted or flattened environment — i.e. essentially every real one. "An environment is a digest" needs the caveat.

### B10. `BuildFromSource` hard-errors, and the build helper lives in a CLI crate. `[spk agent]`
`resolve_runtime_layers` rejects `BuildFromSource` (`exec.rs:191-201`); the handler `build_required_packages` is in `spk-cli-common`. An external resolver either depends on a CLI crate (drags clap/miette), reimplements it, or `set_binary_only(true)` and accepts that some solves succeeding under `spk env` fail under `spfs env`. Decide and document.

---

## Tier 5 — from the two deepest agents (config-API 61 calls, farm-scale 83 calls)

### D1. `Stack::push` promotion and nested-`.unique()` have OPPOSITE dedup semantics — Rule 1 is not well-defined. `[farm, config-API]`
**BLOCKER for Rule 1.** A layer contributed at two levels: as a direct stack entry, `Stack::push` **removes the lower and re-appends at top** (`graph/stack.rs:79-107`); via nested platforms, `.unique()` keeps the **first/bottom** occurrence (`resolve.rs:466-469`). Same conflict, opposite result, depending on arrival path. The promotion case *also* breaks C7's root-first argument directly: a shared base layer re-referenced at the leaf jumps to the top, so bottom flatten groups are no longer identical across shots. The resolver needs an explicit canonicalization pass (first-occurrence-wins, drop later duplicates before building) — it has none.

### D2. `cfg.set()` is O(n) per write with unbounded growth; the per-frame checkpoint example is O(n²). `[config-API]`
**BUG (cost).** "Cheap and safe" is false. Each `set` → `save_state_to_storage` → two `push_tag`s, and `push_tag` **reads and rewrites the entire tag stream file** under a lock (`storage/fs/tag.rs:305-357`). Durable runtimes never truncate the stack, so Python API §8's per-frame `job.frame_done` checkpoint accumulates one layer per frame *and* rewrites a growing tag file every frame. Delete or coarsen that example; writes need write access to the *local* repo, which also isn't stated.

### D3. Config reads silently return the default on storage faults. `[config-API]`
**BUG (correctness).** Both annotation traversals swallow all read errors and continue (`storage.rs:1103-1105,1147-1149`), so a missing/corrupt/unsynced layer yields "key absent" → `cfg.get` returns `default` — *except* a spilled (>16KiB) value with a missing blob, which errors (`storage.rs:1170-1172`). Same logical key: loud above 16KiB, silent default below. A config system that substitutes defaults on storage faults is the exact bug class the suite claims to kill. The blessed reader must distinguish "absent" from "unreadable."

### D4. The concurrency model is worse than "last-writer-wins." `[config-API]`
**GAP.** No CAS anywhere in `read_runtime → mutate → save_runtime`. The loser doesn't lose one annotation — it loses the **whole runtime `Data` blob**: `running`, `owner`, `monitor`, `mount_namespace`, `flattened_layers`, plus any concurrent `spfs commit layer`. Clobbering `monitor`/`mount_namespace` can break teardown, not just config (upstream's monitor reloads-before-save *specifically* to avoid this, `cmd_monitor.rs:183-187`). Winner is decided by wall-clock timestamp then digest-byte tie-break (`tag.rs:79-100`), so clock skew makes the physically-later writer lose. `TagLock` is a 5s no-sleep busy-spin that hard-errors and leaves a stale lockfile wedging all future writes on a killed process (`fs/tag.rs:917-950`).

### D5. Tombstone unset leaks into published identity and resurfaces stale values. `[config-API]`
**GAP.** `commit_platform` clones the whole stack including tombstone layers (`commit.rs:221-234`), so a tombstone permanently hides a key for every downstream consumer of that platform. And re-setting a prior value promotes its existing layer to the top (`Stack::push`), so lowest-wins readers (`spfs info --get`, spk) return a *superseded real value*, not the sentinel. `sources()`/`runtime_sets` also can't be honest — the stack is an ordered set, not a log, and `Status` has no field marking which entries were runtime sets. NUL sentinel prints raw to stdout (`args.rs:510`); use a printable one. Docs contradict each other: guide §4.3 "no unset, sentinel `~`" vs API §4.3 `unset()` with `\x00`.

### D6. `env.digest` / `env.runtime_stack_digest` are unimplementable/unstable as specified. `[config-API]`
**GAP.** `Status` stores no resolved-platform digest (`storage.rs:79-112`); identity is only recoverable if the resolver pushes the platform digest itself rather than expanding it. And `runtime_stack_digest` via `to_platform()` extends the stack with `flattened_layers`, a **`HashSet`**, so its digest varies between processes (`storage.rs:793-797`).

### D7. No admission control or connection reuse — the real frame-0 bottleneck. `[farm]`
**GAP (scale).** `spfs server` has no `concurrency_limit`, no semaphore, no `max_concurrent_streams`, and on EMFILE the accept loop hot-spins (`cmd_server.rs:45-98`). Every payload is a **fresh TCP + HTTP/1.1 handshake, no pool** (`storage/rpc/payload.rs:161-170`). Even a 100%-warm worker makes ≥1 origin `read_object` (`sync.rs:532-543`). `render_manifest` has **no cross-process lock**, so N concurrent jobs on a cold node do N× the hardlink work (`storage/fs/renderer.rs:341-403`; the code comment confirms this is the expected farm case). And pre-rendering per-layer doesn't produce the **flattened group manifests** the runtime actually mounts, whose grouping depends on storage-root path length + username + kernel (`env.rs:1112-1145`) — so "identical groups across shows" doesn't hold across heterogeneous hosts. §3.3's "handled" and §6.1's "operational not architectural" overstate.

### D8. `local repository` node-local vs pool-shared is never stated, and both break something. `[farm]`
**FORCED DECISION.** Renders and runtime state share one `storage.root`. §Scale's inode analysis presupposes sharing; §8.1's per-worker sync presupposes node-local. Shared ⇒ 12k runtime tag files/day + full tag-rewrites under a busy-spin lock on NFS. Node-local ⇒ the `renders/<user>` analysis is moot but every node cold-starts alone. Decide, and redo §Scale for the choice.

### D9. Spec step 14 contradicts §Scale in the same document. `[farm, and general]`
**BUG (self-contradiction).** Step 14 pushes a tag "on every resolution" and calls it "nearly free"; §Scale calls "do not auto-tag every resolution" the single most consequential scale decision; guide §3.5 says "resolution never auto-tags"; §8.1 then tags `jobs/<id>` 6000×/day. A submission *is* an invocation. Four mutually exclusive statements. Pick one; if any auto-tagging survives, cost it against the busy-spin `TagLock` thundering-herd right after each publish.

### D10. Guide §3.5's GC flags are mutually exclusive (clap group), on top of wrong names. `[farm, config-API]`
**BUG.** Beyond the wrong flag names (C9), `--prune-if-older-than` / `--keep-if-newer-than` / `--prune-repeated*` / `--keep-proxies-with-no-links` are all `group = "repo_data"`, which clap defaults to `multiple(false)` — **the whole §3.5 policy cannot be expressed in one invocation** (`cmd_clean.rs:57-90`). The "day-one decision" doesn't compile as written.

### D11. "nothing upstream tests duplicate keys" is FALSE, and shipped `--annotation` precedence contradicts the blessed rule. `[config-API — correction to my own C1 framing]`
`runtime/storage_test.rs:110-124` writes the same key twice and asserts the **first** (lowest) value returns, with a comment calling it intentional. And `cmd_run.rs:261-272` adds `--annotation` pairs in **reverse** order specifically so later flags win *under the lowest-wins reader* — so the design's "later set wins / higher wins" rule contradicts shipped `spfs run --annotation k=1 --annotation k=2` on the very bootstrap path the Python-API build order chooses. The upstream contribution is a test for the **nested-platform BFS** behavior (genuinely untested) and a doc warning, not "the duplicate-key test upstream never wrote."

## Doc/UX gaps (real, lower blast radius)

- **Farm submit writes to the local repo; workers sync from origin. No push step exists in the algorithm.** As written every worker gets `UnknownObject`. Add a push, and decide which repo is canonical for resolver-created objects. `[TD agent]`
- **Stale-context lock silencing:** because the lock key includes the context hash, a stale `$SHOW` mints a *fresh* key and fires no change banner — in the exact "most classic ticket" case. `[artist]`
- **No interactive resolution journal:** after a shell closes, monitor deletes the runtime and the digest exists nowhere; only farm jobs get a tag. Add `~/.spfs-env/history.jsonl`. `[artist]`
- **`diff --mine`, `--reapply`, `--here` are names without specs** (one parenthetical each). `[artist, TD]`
- **Monday base-churn = one banner per shot path, no batch-accept**, training reflexive `--reapply`. `[artist]`
- **Config-dump cost is O(graph)** via bottom-up `all_annotations()` over a 40-65 entry nested stack (50-200 `read_object`s) — read top-down and stop at the first merged blob. `[DCC, spk]`
- **Solve latency vs `cd`-triggered resolution:** spk has automatic mid-solve verbosity escalation and disabled-by-default timeouts — signals a solve is not 200ms. The cache is a *prerequisite* for goal 2, not an optimization. And the explanation/determinism tradeoff: `solve()` gives deterministic order but a bare error; `run_and_log_resolve` gives the rich explanation Rule 2 promises but owns stdout. Pick, or reimplement the formatter. `[spk]`
- **Python `.pyc` on read-only `/spfs`:** promoted pure-Python either recompiles every import or EROFS; shipping `__pycache__` binds the layer to one interpreter, unrecorded. `[TD]`
- **`solution.to_environment()` precedence, `spk install` guard, `ilm/base` churn cadence, service retention tags** — all need explicit statements.

## Correction to the design's biggest hedge: nested runtimes WORK

All three design docs flag nested runtimes as "unverified / test week one." **They are tested and have worked since 2021** — `crates/spfs/tests/integration/unprivileged/test_runtime_recursion.sh:11` runs `spfs run` inside `spfs run`, and the mount path is deliberately nesting-aware (`env.rs:407-433` privatizes an existing `/spfs` "so we don't affect any parent runtime"). Correct the docs. But the test only covers *empty, read-only, shallow* nesting, so name the **real** risks instead: (a) the outer monitor tears the outer runtime down when its tracked pid-set drains (`monitor.rs:170`) — exactly the shape `spfs run A -- spfs run B -- cmd` takes; (b) nesting from an editable/durable outer is untested (durable forces `RenderType::Copy`, no `index=on`); (c) nesting with a real ~65-layer stack is untested; (d) `which_spfs` searches `PATH` first (`resolve.rs:474-489`), so a show layer shipping `spfs-enter`/`-render`/`-monitor` shadows the privileged helpers → user-owned copy → `become_root()` fails confusingly. **Mandate absolute paths for the privileged helpers.**

## What the stress test CONFIRMED works (stop re-checking)

- `spfs env` external-subcommand dispatch ships with zero spfs changes (`bin.rs:98`, `which_spfs`) — but a missing `spfs-env` exits 1 with only a log line, indistinguishable from a real failure; have the Python backend exec `spfs-env` directly.
- Mask layers are fully externally authorable and produce correct **positional** whiteouts (`entry.rs:191,267,363-380`, `env.rs:632-668`); a mask-only layer renders empty and works via upper-dir side effect. Genuinely cheap.
- Forged manifests referencing existing blob digests work with zero payload copy; manifest digests are canonical regardless of HashMap order (`graph/manifest.rs:299`) — better than the design assumed.
- Flattening is automatic on mount, works with a resolver-owned stack, preserves order (`resolve.rs:215-356`).
- `resolve_runtime_layers` is genuinely standalone (no runtime param, `exec.rs:248`); `Solution → digests` path is public.
- `rt.status.stack` is replaced verbatim (the ownership premise is real).
- Repo pinning affects tags only, objects pass through (`storage/pinned/`), so the time-travel *concept* is sound where it can pin.
- `push_tag` skips redundant pushes (self-deduping streams) — though without CAS around resolve-then-insert, concurrent pushes to one non-CI stream can lose the parent link (`storage/tag.rs:219-240`); fine for CI-only `ilm/**`, not for `user/*`/`jobs/*`.
- Precedence *between slots and levels* (as opposed to annotation reads) is correctly specified and code-consistent.

## The short list: what must be settled before any code

1. **Make `promote` produce an spk package** (or add `requires:` + host-option fingerprint to layer metadata). Collapses C8, the rollback gap, and the no-build-env gap at once. Reconciles the safe path (Rule 2) with the advertised path (§5).
2. **Own env composition for real or stop claiming it** (C2): read package ops from specs, compose in the resolver, mask `startup.d`. Otherwise Rule 3 is fiction.
3. **Put the solve's true inputs in the identity** (C3): host options, solver impl, repo pin. Until then "same inputs, same digest" is unkeepable and resolve-at-submit removes spk's ABI protection.
4. **Resolve the annotation read path** (C1): resolver walks the stack itself; kill the `all_annotations()`/`annotation()` recommendations.
5. **Settle GC vs locks** (C5): tag-per-resolution in dedicated namespaces, or an upstream non-tag root, or downgrade locks to change-detection.
6. **Own the tag-name mapping and the config boundary** (S1-S3, B1-B4): resolver validates candidates component-by-component post-substitution, builds its own env-proofed Config, never delegates parsing to spfs. These are security-blocking.
7. **Decide C7** (spk layers vs flatten sharing) and **Decision 1** (versioned paths) — they gate the whole cost model.

## Upstream contributions this surfaced (independent of the resolver)

- The `all_annotations()`/`annotation()` duplicate-key + nested-platform-BFS behavior needs a test and a doc warning (spk relies on the fragile path).
- `TimeSpec` time-only-branch UTC bug (B5); `TimeSpec::parse` empty/multibyte panic.
- `PinnedRepository::address()` un-reparseable `when=` (B6).
- Char-index-as-byte-index panic in tag validation (B1).
- `tag.rs` doc comments claim `:` is legal (it isn't) and versions "must be negative" (type is `u64`).
- The five pre-existing SPFS doc bugs already logged in the design appendix.
