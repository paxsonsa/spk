---
date: 2026-07-30T02:10:00-0700
repository: spk
git_commit: 91f12af6
branch: worktree-spfs-discovery-doc
title: "spfs env: resolved design decisions (post-stress-test)"
status: authoritative
tags: [design, spfs, ilm, environment, decisions]
supersedes_claims_in:
  - 2026-07-29-ilm-env-hierarchy-spec.md
  - 2026-07-29-spfs-env-user-guide.md
  - 2026-07-30-spfs-env-python-api.md
reads_with:
  - 2026-07-30-spfs-env-stress-test-findings.md
---

# spfs env: resolved design decisions

These are owner decisions made after the [stress-test findings](./2026-07-30-spfs-env-stress-test-findings.md), in response to the "what must be settled before code" list. They are authoritative: where an earlier design doc conflicts, this wins. Each records the decision, why, what code fact constrains it, and what it forces downstream.

---

## D-A. Two content mechanisms, kept separate: spk packages vs. promoted layers

**Decision.** There are **two distinct ways content enters an environment, and they do not merge.**

1. **spk packages** — dependency-managed, solved. First-class, unchanged. Anything with dependencies, version ranges, or build/ABI concerns goes here. Enters via `requests:` (Rule 2), one solve.
2. **Promoted / mapping layers** — *unmanaged* files placed at a path. No solve, no dependency graph, no version resolution. Enters via `layers:`/`mappings:` (Rule 1). You own compatibility.

The stress test's proposed fix was to make `promote` *produce* an spk package. **Rejected.** spk packages are their own thing with their own lifecycle (recipes, builds, the solver, `/spfs/spk/pkg` metadata); collapsing promote into that would drag package semantics onto every "just put these files here" action and blur a boundary that should stay sharp.

**Why this is coherent despite finding C8.** C8's segfault (a promoted lib's numpy pin never enters the solve) is real, but the answer is *not* to fake package-ness. The answer is: **promoted layers are explicitly, loudly unmanaged**, and anything that needs dependency resolution is authored as an spk package instead. The tooling's job is to make the boundary impossible to cross by accident, not to erase it.

**What this forces:**

- `promote` output is labeled unmanaged in its metadata and in `--explain`/`why` (`ilm.layer.managed = false`). `why --package X` never attributes a promoted layer's contents to a solve.
- A promoted layer carries a **compatibility fingerprint** (see D-C) so it isn't mounted on an incompatible host, even though it has no dependency graph. This is the narrow, non-solve slice of package-ness that promoted layers *do* need.
- Guidance, enforced in `promote --help` and docs: "shipping a compiled extension or anything with dependencies? make an spk package. `promote` is for configs, data, scripts, and self-contained trees you vouch for."
- Stack assembly keeps them in separate bands (solved spk layers vs. promoted layers), which also feeds D-D's ordering.
- Kills the guide §5.2 contradiction directly: promoted content uses **versioned prefixes** and is understood to be unmanaged; the "release a Python lib with a C extension" flagship becomes "author it as an spk package," and the raw-`promote` path is reserved for pure-Python / data / config where unmanaged is fine.

---

## D-B. C2 stands as the findings state it: the resolver owns env composition, via reading package ops + masking startup.d

No softening. Confirmed by the findings (C2/C2c): `startup.d` runs after the resolver's overrides, spk bakes per-package ops into it, and `--environment-override` isn't even reachable through public API. **Decision:** the resolver reads solved packages' runtime env ops from their specs, composes them (in the band order of D-D) with hierarchy `env:` ops, and **masks `/spfs/etc/spfs/startup.d`** so package scripts never double-apply. This needs the upstream `build_spfs_enter_command` path opened or reimplemented. Flagged here only to confirm it is not reopened by D-A; package *env ops* are read and composed by the resolver even though packages remain a separate content mechanism.

---

## D-C. Host os/arch/distro is part of resolution identity, with an explicit agnostic override

**Decision.** The resolution is **host-qualified by default** and **can be declared host-agnostic per layer/mapping/resolution.**

**Default (host-qualified).** os/arch/distro join the solve options, the cache key, the lock key, the platform-digest identity inputs, and the invocation annotations. This closes finding C3: the same path on rhel8 and rocky9 is two resolutions with two digests, and the ABI mismatch that resolve-at-submit currently hides becomes visible and keyable.

**Grounding.** spk already computes exactly this: `HOST_OPTIONS` = `os` (`std::env::consts::OS`), `arch` (`std::env::consts::ARCH`), and `distro` + `<distro>` version from `/etc/os-release` (`option_map/mod.rs:74-93`). The resolver seeds the solve `OptionMap` from it — the same source `spk` uses, so `spfs env` and `spk` agree on host.

**Override (host-agnostic).** Two levels of opt-out, both explicit:

- **Per resolution:** `spfs env --no-host` (or a config default per probe root) clears host options. Grounds directly on spk's existing `--no-host`, which sets `OptionMap::default()` (empty) instead of `HOST_OPTIONS` (`flags.rs:335-340`). An agnostic resolution has a host-independent digest, so the same digest reproduces on any node — correct for pure-data / config / pure-Python shots.
- **Per layer/mapping:** a spec entry may declare `agnostic: true`, asserting its content has no os/arch dependence (configs, OCIO, pure-Python, docs). Agnostic layers are excluded from the host-qualified part of the digest, so they dedupe across platforms and share one render — recovering the render-sharing that host-qualification would otherwise fragment. Marking a C extension `agnostic: true` is user error the same way a wrong spk build option is; the fingerprint on managed content (D-A) is the backstop.

**What this forces:**

- Cache/lock keys and invocation annotations gain a host block; `status` and `--explain` show it; `--as-of`/`--digest` reproduction records which host options were in force.
- The digest is computed over two partitions: host-qualified layers and agnostic layers. Reproduction on a different host reuses the agnostic partition and re-resolves (or refuses, per policy) the qualified one.
- A promoted layer defaults to host-qualified unless `agnostic: true`; that default is the safe one (a stray `.so` won't silently travel).
- Interacts with D-D: agnostic base/config layers sit at the bottom and stay shared across hosts *and* shows, which is exactly where the flatten-group reuse wants them.

---

## D-D. Ephemeral-by-default runtimes, with loud UX and a first-class commit path

**Decision.** **Resolution never auto-tags.** Most `spfs env` invocations are someone testing something; those runtimes are ephemeral and that is fine and correct. This closes finding C5 (no 6000-tags/day GC pressure, no reproduce-by-digest promise the GC then breaks) — but only if the tooling is honest about it and gives a one-command path to persist.

**The three obligations this puts on the tooling:**

1. **Ephemerality is loud, not fine print.** Every ephemeral resolution says so — a one-line banner at enter (`· ephemeral environment — changes and this exact resolution are not retained; 'spfs env commit' to keep it`), and `spfs env status` states retention class up front. No one should discover ephemerality by losing work.
2. **A first-class commit/promote path.** `spfs env commit` (and the API equivalent) captures the current environment as a persistent artifact: it tags the platform (and commits any live-layer/edit content to real layers first), moving the runtime from ephemeral to retained in one step. This is the "make my test real" verb, and it's the *only* thing that promises reproduce-by-digest.
3. **Locks are change-detectors, with a stated TTL, for ephemeral runs.** An ephemeral resolution's digest is best-effort within the GC window (the hardcoded 15-minute `required_age`, `cmd_clean.rs:184`, is the floor). `spfs env --digest <D>` on a collected ephemeral digest reports "this was an ephemeral environment from <when>; it was not committed and has aged out — nearest committed: <tag> (diff)" rather than a bare `UnknownObject`. Reproduce-by-digest is *promised* only for committed environments.

**Retention classes, made explicit (also settling D9/C9 from the findings):**

| Class | Created by | Tagged? | Retention |
| --- | --- | --- | --- |
| Ephemeral | plain `spfs env` | no | best-effort, GC window |
| Committed | `spfs env commit` | yes, in a durable namespace | until deleted |
| Farm job | submit | yes, `jobs/<id>` in `jobs` **tag namespace** | prune schedule (e.g. 30d) |
| Service | service resolve | yes, `services/<name>/...` namespace | prune schedule |
| Published level | CI | yes, `ilm/**` | permanent, hand-pruned |

Putting `jobs`/`services`/committed-interactive in **separate tag namespaces** is what makes per-class pruning expressible at all — recall prune is repo-wide within a namespace and the §3.5 flags are a mutually-exclusive clap group (findings C9/D10), so retention *has* to be separated by namespace, not by one clever clean invocation.

**What this forces:**

- `spfs env commit` spec: what it captures (stack + committed edits + resolved host block + provenance), what tag namespace it writes, and that it is the sole reproduce-by-digest guarantee.
- The interactive resolution journal (`~/.spfs-env/history.jsonl`, findings gap) still records ephemeral digests client-side so "what did I run an hour ago" is answerable even when the object is gone — the journal points at the commit verb.
- `spfs env status` and `doctor` report retention class and, for ephemeral, the window remaining.
- Spec step 14 ("push_tag on every resolution") is **deleted**; tagging happens only in `commit`/submit/publish.

---

## Net effect on the stress-test blockers

| Finding | Status after these decisions |
| --- | --- |
| C1 (annotation read path inverted) | Still open — resolver-owned top-down stack walk; unaffected by these four, must still be built |
| C2 (startup.d beats resolver) | **Confirmed, owned** (D-B) — read package ops, mask startup.d |
| C3 (digest not a function of inputs) | **Resolved** (D-C) — host-qualified identity + agnostic override |
| C4 (spk replaces stack) | Still open — `ilm.env.owner` marker + upstream guard; D-A keeps packages separate but the guard is still needed |
| C5 (untagged GC'd in 15 min) | **Resolved** (D-D) — ephemeral by design, loud UX, commit path, per-class namespaces |
| C6 (mapping layers not metadata-only) | Partially — D-A/D-C scope mappings to file-granularity, agnostic-by-declaration; the `/spfs/spfs/` prefix + inode-cost fixes are still mechanical work |
| C7 (Decision 4 vs flatten sharing) | Improved by D-C — agnostic base/config layers dedupe at the bottom across hosts and shows; the spk-layer-position tension still needs the explicit call in D-D's band order |
| C8 (promote has no package/ABI metadata) | **Resolved** (D-A + D-C) — dependency content → spk packages; promoted content is loudly unmanaged and host-fingerprinted |
| S1-S3 (namespace escape, env re-point) | Still open — security-blocking; resolver validates candidates post-substitution and builds an env-proofed Config |

Four of the nine top blockers are now closed or owned by decision. The remainder (C1, C4, C6-mechanical, S1-S3) are implementation work with a known shape, not open design questions.

## Still needing a call

- **D-D band order (C7 residue):** where exactly do solved spk layers sit relative to promoted layers and agnostic base? D-C pushes agnostic content to the bottom for sharing; D-A separates managed/unmanaged bands; these mostly agree, but the spk-vs-promoted precedence when both touch a path needs one explicit sentence.
- **C4 guard is upstream:** the `ilm.env.owner` marker plus a `setup_runtime` refusal is a small spk change; confirm appetite for it vs. a resolver-only convention.
- **Agnostic verification:** should CI *check* an `agnostic: true` layer has no ELF/arch-specific content, or is it pure assertion? Cheap check, worth it for managed content.
