---
date: 2026-07-29T23:03:27-0700
repository: spk
git_commit: 3f83ea0e
branch: worktree-spfs-discovery-doc
title: "Hierarchical environment resolution for SPFS: spec file design"
status: draft
tags: [design, spfs, ilm, environment, layers]
supersedes: []
related:
  - ../context/2026-07-26-spk-spfs.md
  - ../context/2026-07-26-spk-spfs-layers.md
---

# Hierarchical environment resolution for SPFS

> ⚠ **SUPERSEDED IN PART — read [`2026-07-30-spfs-env-stress-test-findings.md`](./2026-07-30-spfs-env-stress-test-findings.md) alongside this.** A 7-agent adversarial stress test verified this design against the code and found ~20 load-bearing claims WRONG, two of them security holes. Where the findings doc contradicts this one, the findings doc wins. Highest-impact corrections: the annotation read path is BFS/inverted (C1); the resolver cannot own env composition because `startup.d` runs after its overrides and spk uses it (C2); "same inputs, same digest" is false because the solve depends on unrecorded host options / solver impl / repo state (C3); untagged resolutions are GC'd after 15 min so reproduce-by-digest breaks (C5); mapping layers are not metadata-only and their example paths land at `/spfs/spfs/...` (C6); `..`/empty context vars escape the repo namespace and `SPFS_*` env vars re-point the trusted repo (S1-S3). Do not implement from this document without the findings.

Draft design for a path-driven environment resolver on top of SPFS, plus the spec file format it consumes.

Everything asserted about current SPFS behavior in this document was read out of the source at `3f83ea0e`. Where the existing docs disagree with the code, the code is cited.

## Goals

1. **Hierarchical layering.** Software environments are compositions of base environments (`/sww/gfx`, `/sww/sand`, `/sww/tools`) plus show, context, application, and user overrides.
2. **Directory-level environments.** `cd` into a shot and get that shot's environment, asdf-style. No explicit ref list on the command line.
3. **Point-in-time launch.** Reproduce what an environment was on a given date.
4. **Cheap ad-hoc override.** A developer makes their build available to themselves in seconds, and can promote it to something shareable in one command.
5. **Environment variables and a key/value store** composed along the same hierarchy as the files.
6. **File mapping.** Put content from a source path at a chosen location under `/spfs`.
7. **spk packages with dependencies** as a first-class source of layers.
8. **Pin and lock.** Freeze layers against drift, and get told when a resolution changes.

## Non-goals

- Replacing spk's solver. This resolver accumulates requests and delegates.
- Becoming the source of truth for show configuration. It compiles from whatever already is.
- Making bind-mounted content reproducible. That is not possible and the design says so out loud rather than papering over it.
- Windows support in v1.

## Vocabulary

| Term | Meaning |
| --- | --- |
| **Level** | One directory depth in the hierarchy, e.g. `/show/abc` or `/show/abc/tst/tst0100` |
| **Source slot** | One of the four ordered origins of configuration: base, tag, discovered file, personal |
| **Spec file** | A YAML file in this document's format, either published to a tag or discovered on disk |
| **Mapping layer** | A metadata-only SPFS layer that places existing blob digests at new paths |
| **Request set** | The accumulated set of spk requests across all levels, solved exactly once |
| **Resolution** | The whole process: probe, crawl, accumulate, solve, assemble, produce a platform digest |

## The central insight: three composition rules

The design lives or dies on this. A spec file carries three kinds of declaration and **they do not compose the same way.** Treating them uniformly is the mistake to avoid.

### Rule 1: Stacked (positional)

Applies to `layers` and `mappings`.

Ordered by source slot, then by depth, root to leaf. Higher entries shadow lower ones through ordinary filesystem semantics. This is exactly SPFS `Stack` plus `Manifest::update`, where the merge is last-wins per path and an `EntryKind::Mask` entry acts as a tombstone (`tracking/entry.rs::Entry::update`).

Consequence: conflicts resolve **silently**. Two layers providing `/spfs/bin/maya` produce whichever is higher, with no warning.

### Rule 2: Accumulated, then solved once

Applies to `requests` and `replaces`.

Not positional. Requests are collected from every level into one set, `replaces` prunes inherited requests, and then **a single spk solve runs on the merged set.**

This is not an optimization, it is a correctness requirement. spk requests are version ranges and the solver **intersects** them. So "show wants `maya/2024`" plus "shot wants `maya/2023`" is not an override, it is unsatisfiable. And two independent solves stacked on each other are two internally-coherent sets shadowing each other arbitrarily, which is the exact failure mode spk exists to prevent.

Consequence: conflicts resolve **loudly**, as a solve failure with an explanation. This is strictly better than Rule 1, so prefer `requests` over `layers` wherever a package exists.

### Rule 3: Composed in order

Applies to `env`.

Ops are applied in stack order (base first, personal last), each transforming the accumulated value. `set` replaces, `prepend` and `append` extend with a separator.

Do **not** implement this with `startup.d` scripts. Those are sourced in **alphabetical filename order across the merged `/spfs`** (`docs/spfs/startup.md`), which has no relationship to layer order. A `base.sh` from the bottom layer runs after an `app.sh` from the top. Composition must happen in the resolver, where order is known.

## Source slots

Four slots, fixed order, bottom to top:

| # | Slot | Origin | Time-travelable | Trust |
| --- | --- | --- | --- | --- |
| 1 | **Base** | System config (`/etc/...`), e.g. `ilm/base` | Yes (tag) | Root-owned config |
| 2 | **Tag** | Path-derived tag probe, e.g. `ilm/show/abc` | Yes | Requires repo write access |
| 3 | **Discovered** | Spec files crawled up from cwd | No | Needs an allowlist |
| 4 | **Personal** | `$HOME`, `--with`, env var | No | User's own |

Slot 2 being tag-based is what makes the security story tractable. Injecting into someone's environment through slots 1 and 2 requires repository write access, which is already governed. Only slots 3 and 4 need the file-trust machinery, and slot 4 is the user's own content.

### Interleaving of slots 2 and 3

**Recommendation: depth-interleaved.** At each level, the tag applies, then the discovered file at that same level. So a file at level N beats the tag at level N and everything above it, and loses to the tag at level N+1.

```
ilm/base                              (slot 1)
ilm/show                              (slot 2, level 1)
ilm/show/abc                          (slot 2, level 2)
/show/abc/.spfs-env.yml               (slot 3, level 2)
ilm/show/abc/tst/tst0100              (slot 2, level 4)
/show/abc/tst/tst0100/.spfs-env.yml   (slot 3, level 4)
$HOME/.spfs-env.yml                   (slot 4)
```

The alternative is "all discovered files beat all tags," which is a simpler mental model for whoever placed the file and worse when a deeper tag exists. Marked as **Decision 3** below.

## Path to tag mapping

Tag names are validated by `split_tag_spec` (`tracking/tag.rs`). The org (everything before the last `/`) permits alphanumerics plus `-`, `_`, `.`, `/`. The name (after the last `/`) permits the same minus `/`. Nothing else, so no `@`, no `:`, no spaces, no non-ASCII.

`/show/abc/tst/tst0100/comp` maps to `ilm/show/abc/tst/tst0100/comp`, which parses as org `ilm/show/abc/tst/tst0100` and name `comp`. Fine.

**Do not sanitize illegal characters.** Two distinct directories can sanitize onto the same tag and then silently share an environment. Validate and error instead, naming the offending character and position.

Leading slashes must be stripped. `TagSpec::path()` would otherwise hand a `RelativePathBuf` a leading `/`.

### Probing

Probe **every** candidate level in parallel, not "walk up until a miss." Gaps are legal: `ilm/show/abc` may exist while `ilm/show/abc/tst` does not and `ilm/show/abc/tst/tst0100` does. `resolve_tag` returns `Error::UnknownReference` on a miss, so a miss is cheap.

Five to eight parallel resolves against gRPC is well under the cost of the filesystem crawl it replaces.

## Context and templated candidates

Path derivation is a special case of a more general mechanism: resolution is a function of a **context**, a small map of validated variables, and the path is one *provider* of context among several. This is what lets environment variables and templates drive resolution without becoming a second mechanism.

```toml
# /etc/spfs-env/config.toml
[context.vars.show]
from = ["flag", "env:SHOW", "path"]   # precedence: explicit flag > shell env > extracted from cwd
pattern = "[a-z0-9_-]+"               # strict; substituted values rejecting anything else

[context.vars.role]
from = ["flag", "env:ROLE"]           # a dimension the path cannot express
required = false

[context.path]
patterns = ["/show/{show}/{seq}/{shot}", "/show/{show}"]   # inverse templates: extract vars from cwd

[probe]
candidates = [
  "ilm/base",
  "ilm/show/{show}",
  "ilm/show/{show}/{seq}",
  "ilm/show/{show}/{seq}/{shot}",
  "ilm/show/{show}/role/{role}",
]
```

A candidate whose variables cannot all be resolved is skipped, exactly like a tag-probe miss — no `ROLE` set, no role candidate. This answers the long-open "same directory, different environment per task" question: `--var role=lighting` or `$ROLE` selects a per-role candidate the path could never express.

Rules that keep it safe and explainable:

- **Declared variables only.** The resolver reads exactly the vars named in `context.vars`, never arbitrary environment. A template cannot reference an undeclared variable.
- **Strict value validation, security-relevant.** Substituted values land in tag names, and the tag charset *allows dots* — `SHOW=../../user/mallory` passes a naive charset check and produces a namespace-escaping tag path. Do not rely on downstream normalization: validate each substituted value against a conservative pattern (no dots unless a var opts in), rejecting at substitution time with the offending value named.
- **Substitution only, no logic.** `{var}` interpolation plus `required`/defaults. Conditionals and loops turn config into a program and `--explain` into a debugger. Precedent and cautionary tale in-workspace: spk spec files support full Tera/Jinja2 templating with `env: {}` exposed (`docs/use/create/template.md`, `crates/spk-schema/crates/tera/`), rendered opaquely at file-read time — fine for build-time specs, poison for explainable resolution. Tera is the escalation path if real pressure mounts, with that cost named.
- **Lifecycle split.** Resolution-time substitution is allowed in the candidate list and slot 3/4 files. Published tag content is **static** — CI may template at publish time (git-side, free), but a published platform never contains an unexpanded variable.
- **Context is identity.** Substituted values join the resolution cache key; the lock key becomes `(canonical path, context hash)`; invocation annotations record each variable, its value, and its source (`show=abc from env:SHOW`); `status` and `--explain` print the context block first, because a stale `$SHOW` from a shell opened Tuesday is the single most classic facility support ticket.
- **`--pure`** resolves from flags and path extraction only, ignoring env providers — "show me the canonical answer for this directory."

## File format

```yaml
# Required. Discriminates format and version.
api: ilm/v0/env

# Optional, default true. False stops the upward crawl at this level.
inherit: true

# --- Rule 1: stacked ---

# Existing SPFS refs. Tags or digests. Digests pin, tags follow.
layers:
  - ilm/sww/gfx/2026.1
  - 3YDG35SUMJS67N2QPQ4NQCYJ6QGKMEB5H4MHC76VRGMRWBRBLFHA====

# Place committed content at chosen paths. Compiled to a mapping layer.
mappings:
  - src: /sww/gfx/maya/2024
    dest: /spfs/maya/2024
  - src: /sww/tools/bin/mytool
    dest: /spfs/bin/mytool

# Hide paths from everything below this level. Compiled to a mask layer.
mask:
  - /spfs/bin/legacy-tool
  - /spfs/deprecated/

# --- Rule 2: accumulated, solved once ---

# spk requests. Merged across all levels.
requests:
  - maya/2024
  - nuke/15

# Drop inherited requests for these packages before the solve.
# Required because spk intersects requests rather than overriding them.
replaces:
  - maya

# --- Rule 3: composed in order ---

env:
  - prepend: PATH
    value: /spfs/maya/2024/bin
  - append: PYTHONPATH
    value: /spfs/lib/python
    separator: ":"          # optional, defaults to platform path separator
  - set: MAYA_VERSION
    value: "2024"

# --- Arbitrary metadata, stored as annotations ---
data:
  show: abc
  department: comp

# --- Development only. Not reproducible. See below. ---
bind:
  - src: ./work            # must resolve under this file's own directory
    dest: /spfs/mystuff
```

### Field notes

**`layers`** takes tags or digests. A digest pins; a tag follows. This is the whole pinning mechanism for Rule 1, no extra syntax needed.

**`mappings`** compiles to a metadata-only layer. See "Mapping layers" below. `src` must already be committed to SPFS.

**`mask`** compiles to a layer containing only `EntryKind::Mask` entries. Cheap, one small shared object, reusable across every environment that masks the same path. Prefer this over per-layer include/exclude filters, which multiply renders (see "Scale").

**`replaces`** exists solely because of spk's intersection semantics. Without it there is no way to express "the shot overrides the show's maya pin."

**`env`** ops follow spk's existing schema shape. `crates/spk-schema/src/environ.rs` already defines `EnvOp` with `SetEnv`, `AppendEnv`, `PrependEnv`, each carrying an optional `separator` defaulting to the platform path separator. Reuse that model so spk-built packages and this hierarchy speak the same language.

**`bind`** is deliberately a different key from `mappings`, because the semantics differ in a way users must not confuse:

| | `mappings` | `bind` |
| --- | --- | --- |
| source | committed blob digests | live host path |
| reproducible | yes | **no** |
| shareable | yes | no, host-local |
| has a digest | yes | no |
| cost | metadata only | zero, instant |
| edit-in-place | no | yes |

`bind` maps onto SPFS live layers, which carry the existing constraint that `src` must resolve under the spec file's own directory (`runtime/live_layer.rs::BindMount::validate`).

### Two spec file lifecycles

The same format serves two roles, and not every field means the same thing in both.

**Published to a tag.** Compiled upstream and pushed as a platform. `inherit` is meaningless here; inheritance is resolved at crawl time by the consumer, not baked into a published artifact. `bind` must be **rejected** at publish time, since a bind mount cannot be shared.

**Discovered on disk.** Read live at resolution time. All fields apply.

Validate accordingly. A publish path that silently accepts `bind` produces a tag that works for its author and nobody else.

## Mapping layers

The mechanism for `mappings`, and it is cheaper than it looks.

All the `tracking::Manifest` construction APIs are public (`mknod`, `mkdir`, `mkdirs`, `mkfile`, `to_graph_manifest`) and `tracking::Entry`'s fields are all `pub`, including `object: encoding::Digest`. So a manifest can be built that places **existing** blob digests at **new** paths:

```
1. /sww/gfx is committed to SPFS once      -> manifest M with blob digests
2. walk M, and for each entry to remap:
     dest_manifest.mknod(new_path, Entry { kind, object: <same digest>, mode, .. })
3. dest_manifest.to_graph_manifest()
4. create_layer_from_manifest()            -> a mapping layer
```

No file data moves. The layer is pure metadata over blobs that already exist. It is content-addressed, has a stable digest, and pushes and pulls like any other layer.

Two things to get right. `EntryKind::Blob(u64)` carries the size, so carry it over from the source entry rather than recomputing. And canonicalize the mapping declaration (sort entries, normalize paths) so that two levels declaring the same mapping produce the **same** digest and share a render.

This is the reproducible answer to "put this file here," and it should be the default. `bind` is for the case where you are actively editing.

## Resolution algorithm

```
 1. canonicalize the target path (cwd or explicit argument)
 2. derive candidate tag paths for every ancestor level
 3. probe all candidates in parallel; collect hits            [slot 2]
 4. crawl the filesystem upward for spec files, stopping at
    inherit:false, a root sentinel, or the allowlist edge     [slot 3]
 5. load base config                                         [slot 1]
 6. load personal overrides ($HOME, --with, env var)          [slot 4]
 7. compute the input fingerprint; check the cache
      hit  -> platform digest, jump to 13
 8. accumulate:
      layers/mappings/mask -> ordered list  (Rule 1)
      requests, minus replaces -> one set   (Rule 2)
      env ops -> ordered list               (Rule 3)
 9. compile mappings -> mapping layers; compile mask -> mask layer
10. run ONE spk solve on the request set -> layer digests
11. assemble the Stack (see ordering below)
12. compute the platform digest locally; write_object (idempotent)
13. compose env ops in order -> final variable values
14. if publishing: push_tag(deepest contributing level, digest)
15. create the runtime, set the stack, add invocation annotations
16. write the lock; apply env overrides; exec
```

Step 12 needs no lookup. A platform digest is a pure function of its stack, so compute it locally with `Platform::from(stack).digest()`. `write_object` on an existing digest is a no-op, which makes "create" and "find the existing one" the same operation. There is nothing to search for.

Step 14 is nearly free: `push_tag` already skips redundant pushes when the target is unchanged (`storage/tag.rs`), so pushing on every resolution only grows the stream when something actually changed.

### Stack assembly order

Bottom to top:

```
1. base layers                    (slot 1)
2. solved spk layers              (Rule 2 output)
3. tag + discovered layers        (slots 2/3, depth-interleaved)
4. mapping layers                 (compiled from mappings, same depth order)
5. mask layers                    (must sit above what they mask)
6. personal override layers       (slot 4)
```

Solved spk layers sit low so that explicit overrides can win over solver output. This is **Decision 4** below.

Mask layers are positional: a mask only hides what is beneath it. A mask declared at the show level cannot hide something the shot level adds afterward. That follows from stack semantics and is the correct behavior, but it needs documenting, because "I masked it and it came back" will otherwise be a support ticket.

### Root-first ordering is load-bearing

Ordering root to leaf is not cosmetic. Deep stacks exceed the overlayfs mount option limit (one page) and SPFS responds by flattening layers in groups of seven (`resolve.rs`, `FLATTEN_GROUP_SIZE`). Because common ancestors sit at the **bottom** of every stack, two shots on the same show produce **identical flatten groups** and share those renders. Leaf-first ordering would give every shot a unique flattened manifest and a unique render.

## Environment variable composition

Compose in the resolver, apply as overrides. Never via `startup.d`, for the ordering reason above.

Carry env ops as **annotations on the layers**, so a published tag brings its environment configuration with it:

- Write with `add_annotations` (plural) or a single annotation holding serialized YAML. **Never** `add_annotation` in a loop: each call creates its own layer and pushes it on the stack (`runtime/storage.rs`), so fifty variables becomes fifty stack entries and forces flattening before a single real layer is added.
- Read with `all_annotations()`, **not** `annotation()`. The two disagree. `annotation()` returns the first match walking bottom-up, so the lowest layer wins. `all_annotations()` inserts into a `BTreeMap` while walking bottom-up, so the highest layer wins. You want the latter. All five annotation tests in `runtime/storage_test.rs` use distinct keys, so nothing in the repo pins this down. **Add a test before depending on it.**
- Apply via `spfs-enter --environment-override KEY=VALUE`, which is set-only. The resolver computes final composed values and passes finished strings, which is the right shape anyway since composition then happens in one explainable place.

Note `spfs run` does not expose environment overrides on its CLI. The resolver should do what `cmd_run.rs` does (create the runtime, set the stack, `build_command_for_runtime`, exec) to retain control of `Command.vars`.

Also note: annotations require the FlatBuffers encoding format. `storage.encoding_format = "Legacy"` blocks them entirely, and `cmd_run.rs` errors out rather than silently dropping them.

## spk integration

Two hard facts from `crates/spk-exec/src/exec.rs`.

**spk replaces the stack, it does not append:**

```rust
rt.status.stack = spfs::graph::Stack::from_iter(stack);
```

If the resolver sets a stack and someone then runs `spk env` inside that runtime, the hierarchy's layers are gone, and so are its annotation layers and therefore its provenance. This is a direct ownership conflict over `rt.status.stack`.

Two ways out. Either the resolver owns the final stack and calls spk's solver itself, splicing the solved digests in at step 10 (recommended, no upstream change), or spk learns to compose, which is an upstream change and a larger conversation.

**One layer per resolved package.** A fifty-package solve is roughly fifty stack entries before the hierarchy contributes anything. Budget for flattening on every environment.

**Prior art worth matching.** spk already stores its solve data as a runtime annotation:

```rust
rt.add_annotation(SPK_SOLVE_EXTRA_DATA_KEY, &solve_data, ...)
```

So the annotation-as-metadata pattern is established rather than novel. Namespace resolver keys (`ilm.env.*`, `ilm.src.*`) so they cannot collide with spk's.

## Provenance, digests, and the lock

### Two annotation categories, and why the distinction matters

A subtle trap. Annotations are layers, layers are in the stack, and the stack determines the platform digest. So anything recorded as an annotation becomes part of the digest.

That means **invocation-varying data must not be a platform annotation.** If the resolver stamps a timestamp into the platform, two identical resolutions minutes apart produce different digests, which destroys deduplication and makes the lock useless.

| Category | Examples | In the platform digest? |
| --- | --- | --- |
| **Functional** | env ops, `data` key/values | Yes. They change the environment |
| **Invocation** | timestamp, source file hashes, contributing tag list, live-layer flag, resolving user | **No.** Added to the runtime after the platform is computed and tagged |

Concretely: assemble and digest the functional stack, tag it if publishing, then add invocation annotations to the runtime only.

### The lock

The lock value is the platform digest. Comparing is a digest comparison, and when it differs, `spfs diff <old-platform> <new-platform>` gives the file-level view for free.

Store locks somewhere the **user** controls, keyed by canonical path, e.g. `~/.spfs-env/locks/<hash-of-canonical-path>.lock`. A lock living in the show directory can be rewritten by whoever can rewrite the spec file, and then the alert never fires.

Be clear about what a lock does and does not buy. It detects **change**. It does not establish trust: the first resolution has no lock to compare against, so first use is trust-on-first-use. The lock sits on top of the allowlist rather than replacing it.

### Reproducibility versus time travel

Two different features. Both are needed and they answer different questions.

**Time travel by path** resolves from tags only, against a repository pinned with `?when=@<date>`, and **ignores slots 3 and 4 entirely.** This is a hard rule, not a limitation to apologize for: discovered files and personal overrides have no history, so mixing them in would produce "July 1's tags plus today's files," which answers no question anyone asked. `--as-of` implies `--no-local-overrides`.

Note the pin syntax. `TimeSpec::parse` requires a leading `@` for absolute or `~` for relative. `docs/admin/config.md` lines 66 and 90 show `when = "2020-06-15"` without the `@`, which will fail to deserialize (`TimeSpec`'s `Deserialize` calls `from_str` calls `parse`). Nothing in `time_spec_test.rs` covers the bare form, which is presumably how the docs drifted. Use `when = "@2020-06-15"`.

**Reproduce by digest** takes the platform digest from a lock and runs it, regardless of how messy the original resolution was. This is what people actually want after a failed render, and it works even when slots 3 and 4 contributed.

**Except when `bind` was involved.** A bind mount has no digest and its contents at that moment are unrecoverable. Record a live-layer flag in the invocation annotations, and when someone asks to reproduce such a runtime, say so plainly. "That environment bind-mounted `/home/x/work`, whose contents at that time are not recoverable" is a far better answer than silently handing over something different. This is the most likely source of "the tool lied to me," and it costs almost nothing to handle honestly.

## Security model

Slot ordering does most of the work. Slots 1 and 2 require repository write access, which is already governed. The file-trust machinery only has to cover slots 3 and 4.

For slot 3 (discovered files):

- **Path allowlist** in system config. `/show/**` yes, `/tmp/**` and `/var/tmp/**` no.
- **Ownership and mode checks.** Owned by root or a trusted gid, and not group or world writable.
- **Boundary markers.** `inherit: false`, a root sentinel file, or the allowlist edge, whichever comes first.
- **Canonicalize before matching.** `/show/projects/...` is likely automounted or symlinked, so the logical and physical ancestor chains differ. Canonicalize (which is what live layer validation already does) and make sure the allowlist covers physical paths.

One trap to avoid. **Security-relevant settings must not be readable from the environment.** SPFS's own `load_config` adds file sources first and then `Environment::with_prefix("SPFS")`, so `SPFS_*` variables override `/etc/spfs.toml`. Anything the resolver relies on for trust must come from its own fixed system location with no environment override, or an attacker relaxes the constraint by setting a variable.

Git's `safe.directory`, added after CVE-2022-24765, is worth reading for how they scoped a near-identical problem.

## Scale

Numbers below are reasoned from the code, not measured against a real facility repository.

**Tags: fine, with one caveat.** One append-only `.tag` file per stream in a nested tree. Thousands of small files is unremarkable, and `push_tag` skips redundant pushes so streams grow only on real change. The caveat: `Cleaner` must walk the **entire** tag tree, since tags root the GC. That walk scales linearly with tag count and is the slow part of a clean on NFS. `with_tag_stream_concurrency` parallelizes it but cannot avoid it.

**Therefore: do not auto-tag every resolution.** Tag on publish. Record digests in locks and invocation annotations otherwise. A tag per shot per day is a very different repository from a tag per invocation. This is the single most consequential scale decision in the design.

**GC: better than feared.** `Cleaner` memoizes with `attached: DashSet<Digest>` and short-circuits on revisit, so each object is traversed once regardless of how many tags reach it. Mark cost is O(distinct objects), not O(tags × layers). Since the hierarchy shares base layers across every show, the distinct count is far below the naive product.

**Concurrent resolution is the real bottleneck.** Thousands of farm jobs each doing five to eight tag resolves is tens of thousands of requests arriving at one `spfs server` in a burst at job start. Design for **resolve once at submit, pass the digest to jobs.** Jobs then do zero probing, zero crawling, zero solving. If jobs must resolve themselves, the cache is load-bearing rather than an optimization.

**Cache key.** `(canonical contributing file set + content hashes + resolved tag targets + override set)`. The resolved targets matter: a platform digest depends on resolved layer digests, so if `maya/2024` moves, the digest changes even though no spec file did. A cache keyed only on file hashes goes stale silently and hands people yesterday's maya. Either include resolved targets or give the cache a deliberate, documented TTL.

Useful property: because the key is the **contributing file set** rather than the cwd, many directories collapse onto one entry. `/show/abc/tst/tst0100/comp` and `/show/abc/xyz/xyz0200/anim` share a cache entry when neither has its own spec file.

**Renders and inodes: the sleeper problem.** `RenderStore::for_user` puts renders under `renders/<username>/` with a `proxy` subdirectory, so the directory structure is duplicated per user while files are hard links. A thousand artists times forty layers is real inode pressure. `allow_payload_sharing_between_users` mitigates the payload side, and `spfs clean` already removes proxies with no remaining links **by default** (`--keep-proxies-with-no-links` opts out), so that reclamation is on unless someone turned it off.

The thing that would blow this up is **per-shot layer filtering.** Every distinct include/exclude combination is a new manifest, a new render, per user. This is why `mask` (one shared object) is in the format and per-layer filters are not. If filters are added later, canonicalize them so identical filters dedupe.

**Annotation layers.** Covered above: batch them, or stack depth explodes.

## Migration and coexistence

The requirement to keep the existing system intact and the requirement that this be easy to adopt push in the same direction: **the resolver should be a compiler, not a new source of truth.**

If it can read how sww environments are defined today and emit layers plus env ops, then nobody rewrites anything to get value, environments flip over one at a time, and the old system stays authoritative throughout.

**The differ is the feature that makes migration possible.** Given a path, produce the old environment and the new one, then diff both sides: the file set (which is `spfs diff` once both are platforms) and the composed environment variables (which is the resolver's own data). Run it across a few hundred real shot paths, and the discrepancy list is the migration plan. Without it, shows are being asked to trust a rewrite on faith.

**Shadow mode** is the same idea at runtime. The new resolver runs and records what it would have produced; the old system still drives the environment. Log deltas, flip when the log is boring.

**Test nesting early.** During migration a tool launched from an old-style environment will want to enter a new-style one. SPFS has `spfs-join` for attaching to an existing runtime, but whether a runtime can be entered from **inside** another runtime is untested here and unverified. If nesting does not work, that reshapes the rollout sequence, so find out before planning around it.

## Command surface

Four verbs, because five hierarchy levels plus env op composition means people need to be able to ask what happened.

```sh
spfs-env .                      # resolve cwd and enter
spfs-env /show/abc/tst/tst0100  # resolve an explicit path
spfs-env --explain .            # which source contributed which layer and which env op
spfs-env --as-of @2026-07-01 .  # tags only, no local overrides
spfs-env --digest <DIGEST>      # reproduce from a lock
spfs-env promote ./work         # commit a bind mount to a tagged layer
```

**Naming: it cannot be `spfs enter`.** There is no `Enter` variant in SPFS's clap `Command` enum, so `spfs enter .` falls through to the external-subcommand handler, which calls `which_spfs("enter")` and finds `spfs-enter`, the privileged binary that creates the mount namespace. It would receive `.` as an argument.

That same external-subcommand fallback is how this should ship. A binary named `spfs-env` on `PATH` makes `spfs env ...` work with **no changes to SPFS at all**.

`--explain` is the verb most likely to be underestimated. With four source slots, depth interleaving, one spk solve, and ordered env composition, it is the difference between a tool people trust and a tool people route around.

## Decisions needed

These change the design rather than the tuning. Each needs an answer before implementation.

**Decision 1: Are `/sww` layers installed to versioned or unversioned paths?**

If `/sww/gfx` yields `/spfs/maya/2024/bin/maya` and `/spfs/maya/2023/bin/maya`, then "override the maya version" is a `PATH` problem. Both layers coexist, nothing conflicts, and Rule 3 does the work. Layer removal is unnecessary for the common case.

If layers install to `/spfs/bin/maya`, then adding maya-2023 over maya-2024 leaves 2024's files wherever 2023 has no file at the same path. That is the silent-breakage case and `mask` becomes mandatory rather than optional.

This determines how much of the format is actually needed, so answer it first.

**Decision 2: Do path-derived tags hold that level's contribution, or a fully resolved platform?**

Per-level contribution (assumed throughout this document) means a change to `ilm/show` propagates to every shot with no republishing. The cost: that change alters every shot's platform digest at once, invalidating every cache entry and firing every lock alert in the facility. Correct, but noisy, and it makes `--explain` load-bearing.

Fully resolved means one lookup after probing and no stacking, but every level must be republished when any ancestor changes. That is a build system to operate.

**Decision 3: Depth-interleaved, or do discovered files always beat tags?**

Recommendation above is depth-interleaved. The alternative is a simpler mental model for whoever places a file, and worse when a deeper tag exists.

**Decision 4: Where do solved spk layers sit in the stack?**

Assumed low, so explicit overrides win. Needs to be a decision rather than an emergent property.

**Decision 5: How much of current ILM resolution is environment variable manipulation versus file provisioning?**

If it is mostly environment variables, Rule 3 is the hard part and should be designed first, since it will constrain the format more than the layering does. This also feeds Decision 1.

**Decision 6: Is there an existing source of truth for show environment configuration?**

If shows are already configured in Qi, a git repository, or a database, then tags are a published artifact of that and this format is largely a compilation target. That is a materially easier problem, and it moves the security question from "who can write to a show directory" to "who can run the publisher."

**Decision 7: When context providers disagree, which wins and how loudly?**

`env:SHOW=def` in a stale shell versus `show=abc` extracted from the cwd. The declared precedence (`flag > env > path`) says env wins, which matches how facility tooling behaves today but silently honors stale shells. Options: warn-and-env-wins (default proposed), path-wins, or hard error. The conflict is mechanically detectable, so this is pure policy — but it determines the single most common support interaction, so it deserves an explicit call and probably a per-facility config knob.

## Build order

1. Path-to-tag mapping and validation, plus the probe. Small, testable, no filesystem involvement.
2. The upward crawl with boundaries and the allowlist. Table-driven tests over a fixture tree, including the edge cases: `cd /`, no spec anywhere, `$HOME` inside a crawled root, symlink loops, deleted cwd.
3. Rule 1 accumulation and stack assembly. Emit an EnvSpec, hand it to `spfs run`, confirm environments mount.
4. Mapping layer compilation. Verify digests are stable across levels declaring identical mappings.
5. Rule 3 composition and `--environment-override` application. Add the `all_annotations()` precedence test first.
6. Rule 2 and the spk solver call, including `replaces`.
7. Lock, invocation annotations, `--explain`.
8. The differ, then shadow mode.

Steps 1 through 3 are enough to prove the ergonomics on a real show path, which is worth doing before committing to the rest of the format.

## Appendix: existing behavior relied upon

| Behavior | Location | Note |
| --- | --- | --- |
| External subcommand fallback | `spfs-cli/main/src/bin.rs`, `resolve.rs::which_spfs` | Ships this tool with no SPFS changes |
| Tag name charset | `tracking/tag.rs::split_tag_spec` | Alphanumeric, `-`, `_`, `.`, plus `/` in org |
| `push_tag` skips redundant pushes | `storage/tag.rs` | Free tagging on every resolution |
| Platform digest is a pure function of its stack | `graph/platform.rs`, `graph/object.rs` | No lookup needed |
| `write_object` is idempotent | content-addressed | "Create" and "find" are the same call |
| Public manifest construction, `pub` Entry fields | `tracking/manifest.rs`, `tracking/entry.rs` | Mapping layers |
| `EntryKind::Mask` tombstones and whiteouts | `tracking/entry.rs`, `env.rs::mask_files` | `mask` field |
| Group-of-7 flattening | `resolve.rs` | Why root-first ordering matters |
| `Cleaner` memoizes traversal | `clean.rs`, `attached: DashSet` | GC scales on distinct objects |
| Repo pinning via `?when=` | `config.rs::RemoteConfig::open` | Time travel |
| `EnvOp` / `Set` / `Prepend` / `Append` with separator | `spk-schema/src/environ.rs` | Reuse, do not reinvent |
| spk stores solve data as an annotation | `spk-exec/src/exec.rs` | Prior art |
| Live layer source confinement | `runtime/live_layer.rs` | `bind` safety |

### Known documentation bugs in this area

Do not trust these while working here.

1. `runtime/storage.rs:525` and `:562` comment that an annotation layer is "added to the bottom of the runtime's stack." `push_digest` calls `Stack::push`, which adds to the **top**.
2. `docs/admin/config.md:66` and `:90` show `when = "2020-06-15"`. `TimeSpec` requires a leading `@`, so this fails to deserialize.
3. `docs/spfs/develop/runtime.md` refers to `spfs-enter` calling back into `spfs init-runtime`. No such command exists.
4. `docs/spfs/develop/design.md` claims two storage backends. There are five plus a pinned wrapper.
5. Nothing in `docs/spfs/` documents automatic layer flattening, which is the most surprising behavior in the layer system.
