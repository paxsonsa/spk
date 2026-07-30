---
date: 2026-07-29T23:43:25-0700
repository: spk
git_commit: dbcab9ce
branch: worktree-spfs-discovery-doc
title: "spfs env: user guide (design fiction)"
status: draft
tags: [design, spfs, ilm, environment, user-guide]
related:
  - 2026-07-29-ilm-env-hierarchy-spec.md
  - ../context/2026-07-26-spk-spfs-layers.md
  - ../context/2026-07-26-spk-spfs.md
---

# spfs env — user guide

> **Status: design fiction.** This guide is written as if `spfs env` v1 has shipped, because walking real personas through real workflows is the fastest way to find the holes. Everything under `spfs env ...` is **proposed**. Everything under plain `spfs ...` (run, info, diff, log, commit, tag, pull, push) **exists today**. The architecture behind this guide is in [the design doc](./2026-07-29-ilm-env-hierarchy-spec.md); where this guide hits a problem, it says so inline in a `⚠` block and states the answer the design gives (or fails to give).

> ⚠ **STRESS-TESTED — many concrete commands here are WRONG. Read [`2026-07-30-spfs-env-stress-test-findings.md`](./2026-07-30-spfs-env-stress-test-findings.md).** The two flagship workflows both fail as written: §5's `promote` produces a raw layer with no package/ABI metadata, so a released lib's numpy pin never enters the solve and segfaults on the farm (C8); §8's farm submit writes objects to the *local* repo while workers sync from *origin* — every worker gets `UnknownObject` (no push step). §3.5's GC command uses non-existent, mutually-exclusive flags (C9/D10). §5.2's unversioned `promote --dest` contradicts §6.1. §4's config store rests on a read path the code inverts (C1). The core model holds; the specific verbs and recipes do not.

---

## 1. The mental model, in five minutes

Three ideas carry the whole system.

**1. An environment is a digest.** Every resolution produces a platform digest. Same inputs, same digest, always. The digest is the environment's identity: it's what you lock, what you diff, what you hand to the farm, and what you paste into a support ticket. Names (`ilm/show/abc`) are conveniences that point at digests and can move; digests never do.

**2. Configuration comes from four slots, in fixed order.**

```
4  personal   ~/.spfs-env.yml, --with, $SPFS_ENV_WITH        (yours, wins)
3  discovered .spfs-env.yml files crawled up from cwd        (in-place overrides)
2  tags       ilm/base, ilm/show/abc, ... probed per level   (published, governed)
1  base       system config                                  (facility floor)
```

Slots 1 and 2 are time-travelable and trusted (writing them requires repo access). Slots 3 and 4 are neither, and the tooling never pretends otherwise.

**3. Three kinds of declaration, three composition rules.** Files stack (higher shadows lower, silently). Package requests accumulate and get solved once (conflicts fail loudly, with an explanation). Environment variables compose in stack order (prepend/append/set). Knowing which rule applies is 90% of predicting what you'll get.

### The thirty-second version

```console
$ cd /show/abc/tst/tst0100/comp
$ spfs env
· probed 5 levels: ilm/base ilm/show ilm/show/abc ilm/show/abc/tst/tst0100
· found 1 override file: /show/abc/tst/tst0100/.spfs-env.yml
· solved 34 packages, assembled 41 layers
· platform: LQ7XN2AWZJS67QP4NQCYJ6QGKMEB5H4MHC76VRGMRWBRBLFHA====
[abc/tst0100/comp] $ which nuke
/spfs/app/nuke/15.1/bin/nuke
[abc/tst0100/comp] $ spfs env config get show.colorspace
acescg
```

You cd'd somewhere and got that place's environment. That's the whole pitch. Everything else in this guide is what happens when 4,000 people do it at once.

---

## 2. The spec file

One format, two lifecycles: published to a tag (compiled by CI, becomes slot 2) or dropped in a directory (read live, becomes slot 3).

```yaml
api: ilm/v0/env
inherit: true                  # false stops the upward crawl here

layers:                        # Rule 1: stacked. tags follow, digests pin.
  - ilm/sww/gfx/2026.06
  - ilm/tools/fxutils/1.4.0

mappings:                      # Rule 1: put committed content at a path. zero-copy.
  - src: /sww/tools/ocio/configs/abc
    dest: /spfs/etc/ocio

mask:                          # Rule 1: hide paths from everything below this level
  - /spfs/bin/legacy-comp

requests:                      # Rule 2: spk requests, merged across levels, solved ONCE
  - nuke/15.1
  - ilm-comp-tools/3

replaces:                      # Rule 2: drop an inherited request before the solve
  - nuke                       # (spk INTERSECTS ranges; without this, show=15.0
                               #  + shot=15.1 is a solve failure, not an override)

env:                           # Rule 3: composed in stack order
  - prepend: PATH
    value: /spfs/app/nuke/15.1/bin
  - set: OCIO
    value: /spfs/etc/ocio/config.ocio

config:                        # key/value store, carried as annotations,
  show.colorspace: acescg      # readable inside the runtime by any program
  review.burnin: "on"

apps:                          # app profiles: merged only with --app <name>,
  nuke:                        # composed along the SAME hierarchy as everything else
    requests: [nuke-survival-toolkit/2]
    env:
      - prepend: NUKE_PATH
        value: /spfs/show/abc/nuke

bind:                          # DEV ONLY: live bind mount, NOT reproducible,
  - src: ./tools               # rejected at publish time
    dest: /spfs/showtools
```

> ⚠ **Problem: two files, one suffix, silent misparse.** The `api:` field is the only discriminator, and today's spfs default (`SpecApiVersion::default() = V0Layer`) means a file *missing* `api:` parses as a live layer. **Answer:** `ilm/v0/env` has no default. A file without `api:` is a hard parse error naming the missing field. Cheap to enforce, saves an afternoon of confusion per TD per year.

---

## 3. For the environment team: building thousands of these

You own slots 1 and 2: the shared tree that everyone else layers on.

### 3.1 The naming scheme

Tag names allow alphanumerics plus `- _ .` (and `/` as the path separator). No `@`, no `:`, no spaces. Recommended tree:

```
ilm/base                              the facility floor (moving)
ilm/sww/gfx/2026.06                   base env releases (immutable, dated)
ilm/sww/gfx                           moving pointer to current release
ilm/app/maya/2024.2                   DCC layers (immutable per version)
ilm/tools/<name>/<version>            released tools (immutable)
ilm/show/<show>                       per-level contributions (moving)
ilm/show/<show>/<seq>/<shot>          ...
user/<login>/...                      personal namespace (yours)
jobs/<jobid>                          farm retention tags (pruned on schedule)
```

Two kinds of tags, on purpose. **Moving tags** (`ilm/show/abc`) are what the resolver probes; their history lives in the tag stream (`spfs log ilm/show/abc`, `ilm/show/abc~1` for one change back). **Immutable tags** (`ilm/app/maya/2024.2`) are for things that must never change meaning. Never re-push an immutable tag; CI should refuse.

### 3.2 The publish pipeline

Spec sources live in **git**, not in the repo directory tree. CI compiles and publishes:

```console
$ git clone ilm/env-config && cd env-config
$ vi shows/abc/env.yml
$ git push origin feature/abc-nuke-bump      # → PR → review → merge
# CI on merge:
#   spfs env publish shows/abc/env.yml --level ilm/show/abc
#   · compiles mappings + mask → layers, bakes env/config → annotations
#   · rejects bind:, validates requests against the repo
#   · computes platform digest, write_object (idempotent)
#   · push_tag ilm/show/abc → new stream version only if digest changed
```

This answers the security question by construction: only CI's service account can write `ilm/**` tags, so slot 2 is exactly as trustworthy as your PR review. It also gives you rollback for free:

```console
$ spfs log ilm/show/abc
 J7MK3... ilm/show/abc    ci@publish  2026-07-28 14:02
 X2PQ9... ilm/show/abc~1  ci@publish  2026-07-21 09:15
$ spfs tag ilm/show/abc~1 ilm/show/abc      # revert = re-tag the old version
```

> ⚠ **Problem: base churn is a facility-wide event.** Change `ilm/base` and every environment's digest changes at once: every cache invalidates, every lock alert fires, every first launch re-renders. **Answer:** you can't avoid it, so schedule it. CI runs the differ (§10.4) against a sample of shot paths before publishing any level above `show`, posts the delta report to the review, and publishes base/sww changes on a known cadence, not at 4pm on a Friday.

### 3.3 Keep levels shallow: nest platforms

A level's tag points at a **platform**, and platforms expand recursively at resolve time. Use that. `ilm/sww/gfx/2026.06` should be one platform of 15 layers, not 15 entries in every consumer's spec. Consumers write one line; the stack machinery flattens the nesting; and because shared ancestors sit at the bottom of every stack, the automatic group-of-7 flattening produces **identical groups across shows**, so renders get reused instead of multiplied.

> ⚠ **Problem: stack depth.** base(1 platform→12 layers) + sww(15) + show(4) + shot(2) + solve(30 packages = 30 layers) + masks + annotations ≈ 65 entries, well past the overlayfs one-page mount-arg limit. **Answer:** this is handled (flattening), but it's *your* job to keep it cache-friendly: root-first ordering, nested platforms for the stable bottom, and batched annotations (one layer, not one per key). Watch `spfs env doctor`'s flatten stats; if every shot flattens differently, something in your level ordering is defeating reuse.

### 3.4 Annotation schema governance

Everyone's programs read the same key space, so treat it like one. A `schema/` directory in the env-config git repo declares known keys, types, and owners:

```yaml
# schema/show.yml
show.colorspace: {type: string, owner: color-sci, doc: "working colorspace"}
review.burnin:   {type: enum(on,off), owner: review-tools}
```

CI validates published specs against the schema; unknown keys warn, type mismatches fail. Namespaces by prefix: `show.*`, `review.*`, `app.<dcc>.*`, `dev.*` (never validated), `spk.*` (reserved, spk already writes its solve data there).

### 3.5 GC policy, decided on day one

Tags root the GC. The design's rule: **resolution never auto-tags.** Tags come from publishes (CI), promotions (deliberate), and job submission (`jobs/<id>`, pruned). With that, tag count grows with *decisions*, not with *invocations*, and `spfs clean` stays tractable:

```console
$ spfs clean --prune-repeated-tags 50 \
             --prune-tags-older-than 90d \
             --keep-tags-newer-than 14d \
             --dry-run
```

> ⚠ **Problem: locks don't protect objects from GC.** A lock file records a digest, but the Cleaner only keeps what tags reach. An interactive resolution that was never tagged gets collected once it ages past the clean window, and then `spfs env --digest <old>` fails with unknown object. **Answer:** be honest about the two retention classes. Farm submissions tag `jobs/<id>` (retention = your prune schedule, e.g. 30 days). Interactive locks are best-effort within the clean age window, and the tooling says so: `spfs env --digest` on a collected digest reports *"this environment aged out of retention on <date>; the nearest surviving publish is ilm/show/abc~2 (diff attached)"* instead of a bare error. If a TD needs an environment kept forever, that's one command: `spfs tag <digest> ilm/show/abc/keeps/2026-07-29`.

---

## 4. Program configuration: annotations as a settings store

Programs inside a runtime need settings: colorspace, plugin paths, feature flags, review options. The `config:` section rides in the platform as annotations, which buys you something subtle: **config is part of the digest.** Two environments with different settings are different environments, automatically, with no "same binaries, mystery behavior" class of bug.

### 4.1 Reading config

```console
[abc/comp] $ spfs env config get show.colorspace
acescg
[abc/comp] $ spfs env config list --sources
show.colorspace = acescg      ilm/show/abc
review.burnin   = on          /show/abc/tst/tst0100/.spfs-env.yml   [override]
app.nuke.gpu    = auto        ilm/base
```

From Python, until there's a proper client library:

```python
import subprocess, functools

@functools.cache
def env_config(key, default=None):
    r = subprocess.run(["spfs", "env", "config", "get", key],
                       capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else default
```

For programs that can't shell out (or run before anything can), an `env:` op exporting the value as a variable is the escape hatch. Config for things that read config; variables for things that read variables; the spec file supports both and `--explain` traces both.

### 4.2 How precedence works (and the trap the resolver hides)

The resolver composes all `config:` sections in slot/depth order at resolve time and writes **one merged annotation** (`ilm.cfg`, serialized YAML) onto a single synthetic layer at the top of the stack. `config get` reads the merged blob. Per-source values are kept in a second annotation for `--sources` and `--explain`.

This is deliberate armor around a real footgun in spfs today: the two raw annotation readers disagree on precedence (`annotation()` = lowest layer wins; `all_annotations()` = highest wins; untested either way upstream). Programs should never touch raw annotations for config; they get one blessed read path with one documented rule: **higher slot wins, deeper level wins, later `--with` wins.**

### 4.3 Rules of thumb

- **Keep values small.** ≤16KiB stays inline; bigger values spill to a blob (works, but every read is a payload fetch). A 2MB OCIO config is a **file in a layer**, and `config` holds its path.
- **Strings only.** Structured values are YAML inside the value. Binary data has no business here.
- **No unset, by design.** A deeper level can override a key but not delete it. Deleting a key someone's program reads is an API break; make it explicit with a sentinel (`~`) and schema review.
- **Never put invocation data in `config:`.** Timestamps, usernames, hostnames → different digest per invocation → dedup destroyed, locks useless. Invocation metadata goes on the *runtime* (§10.2), not the platform. CI lints for known-volatile keys.

---

## 5. Pipeline TD cookbook: releasing a Python library

The most common workflow in the building, so it gets the fewest steps. You have `fxutils`, a Python package, and comp on `abc` needs it.

### 5.1 Develop against the real environment

```console
$ cd ~/dev/fxutils
$ cat > .spfs-env.yml <<EOF
api: ilm/v0/env
bind: [{src: ./python, dest: /spfs/ilm/lib/python-dev}]
env:  [{prepend: PYTHONPATH, value: /spfs/ilm/lib/python-dev}]
EOF
$ spfs env /show/abc/tst/tst0100/comp --here
· note: this runtime includes 1 live bind mount → NOT reproducible
[abc/comp+dev] $ python -c "import fxutils; fxutils.selftest()"
```

(`--here` = resolve the given path's environment but also honor the cwd's spec file. Edit-test loop with no commits: the bind mount is live.)

### 5.2 Promote: one command from "works for me" to "exists for everyone"

```console
$ spfs env promote ./python \
    --dest /spfs/ilm/lib/python \
    --env 'prepend:PYTHONPATH=/spfs/ilm/lib/python' \
    --tag ilm/tools/fxutils/1.4.0
· committed 214 files (9 new payloads, 205 deduped)
· baked env ops into layer annotations (self-describing)
· tagged ilm/tools/fxutils/1.4.0 → GXT4A...====
```

**Self-describing layers** are the point of `--env` here: the PYTHONPATH op travels *inside* the layer as an annotation. Whoever adds this layer anywhere gets the path composed automatically. The historical failure mode this kills: "I added the lib but forgot the env var in the wrapper," discovered at 11pm before a delivery.

### 5.3 Ship it to the show

```console
$ cd ~/dev/env-config
$ $EDITOR shows/abc/env.yml        # layers: + ilm/tools/fxutils/1.4.0
$ git commit -am "abc: fxutils 1.4.0" && git push   # → PR → CI publishes
```

Anyone can test the exact change *before* the merge:

```console
$ spfs env /show/abc/tst/tst0100/comp --with ilm/tools/fxutils/1.4.0
```

Hotfix flow is the same three steps with a patch version. Rollback is a one-line git revert, and CI re-publishes the previous digest (which still exists; content-addressing means "republish old" is a tag move, not a rebuild).

> ⚠ **Problem: two TDs publish to the same show at once.** **Answer:** git is the serialization point. The repo tag is only ever written by CI from merged main, so the race collapses to a merge conflict in a YAML file, which is a solved problem.

---

## 6. DCC layering: Maya, Nuke, Houdini

### 6.1 Versioned prefixes, and why this guide insists

This guide takes a position on the design doc's Decision 1: **DCC and tool layers install into versioned prefixes** (`/spfs/app/maya/2024.2/...`), never bare (`/spfs/bin/maya`, `/spfs/lib/...`).

With versioned prefixes, "the shot uses Maya 2024.2" is an *activation* (env ops point PATH at one prefix), and two Maya versions coexist in one stack without conflict. Overriding a version touches Rule 2 (`replaces: [maya]`) and Rule 3, both loud and traceable. With bare prefixes, overriding means layering 2024.2's tree over 2023's and hoping the shadowing is total; every partial overlap is a silent mixed install. The `mask` field exists for the leftovers, but the design goal is that DCCs never need it.

Vendors that insist on fixed paths get wrapped at layer-build time: install into the versioned prefix, and if the app hard-codes `/usr/autodesk/...`, a mapping entry or relocation shim goes in the app layer itself, authored once by the env team instead of per-show.

### 6.2 App profiles: the same hierarchy, per application

Apps aren't directories, so they can't be levels, but every level can contribute to a named profile. `--app maya` merges each level's `apps.maya` fragment in the same depth order as everything else:

```yaml
# ilm/base
apps: {maya: {requests: [maya/2024], env: [{prepend: PATH, value: /spfs/app/maya/2024.2/bin}]}}
# ilm/show/abc
apps: {maya: {requests: [mtoa/5.4, abc-maya-shelves/2],
              env: [{prepend: MAYA_MODULE_PATH, value: /spfs/show/abc/maya/modules}]}}
# shot override file
apps: {maya: {replaces: [mtoa], requests: [mtoa/5.3]}}   # this shot pins an older MtoA
```

```console
$ spfs env --app maya /show/abc/tst/tst0100/anim -- maya
```

One solve still runs (base's maya + show's mtoa + shot's replace, together), so an incompatible plugin pin fails at resolve with spk's explanation instead of failing at Maya startup with a stack trace.

Launchers stop being 400-line wrapper scripts. The wrapper *is* `spfs env --app maya "$SHOT" -- maya "$@"`, and everything it used to compute lives in specs where `--explain` can see it.

> ⚠ **Problem: first launch renders a 9GB Maya layer.** Layers render on first local use; artists interpret the one-time 90 seconds as "the new system is slow." **Answer:** operational, not architectural. Workstation pools pre-pull and pre-render app layers on release (`spfs pull ilm/app/maya/2024.2` + render warm in the overnight window); `spfs env doctor` shows render-cache hit state so support can tell "cold cache" from "actual problem" in one command.

---

## 7. Developer tooling: your environment, anywhere

Personal tools ride the same machinery under the `user/<login>/` namespace, and the repo makes them portable: any machine that can reach the repo can reproduce your toolchain.

```console
$ spfs env promote ~/tools/bin --dest /spfs/user/apaxson/bin \
    --env 'prepend:PATH=/spfs/user/apaxson/bin' \
    --tag user/apaxson/devtools
$ cat ~/.spfs-env.yml
api: ilm/v0/env
layers: [user/apaxson/devtools]
```

Now every `spfs env` anywhere puts your tools on top (slot 4). On a render node, a colleague's machine, a fresh workstation: same tag, same tools.

**Project toolchain pinning** is the same trick pointed at a team. A repo's checked-in `.spfs-env.yml` with a *digest* (not a tag) gives every contributor and CI the identical toolchain, immune to anything moving:

```yaml
api: ilm/v0/env
layers: [BQ2MN...====]    # rust 1.88 + clang 19 + just + mold, promoted once
```

That's the devcontainer use case with no Docker daemon, facility-native, and diffable (`spfs diff OLD NEW` when someone bumps the pin).

Two honest limits. **Secrets never go in layers**: repo read access is broad by design, so credentials stay in the host environment (`environment.variable_names_to_preserve` carries them through). **Personal ≠ shared**: slot 4 is invisible to the farm and to published resolutions by construction, so "works on my machine because of my slot-4 layer" is a class of bug; `spfs env diff --mine` (§10) exists precisely to expose it in ten seconds.

---

## 8. Farm and services: dynamic environments at job scale

### 8.1 The one rule: resolve at submit, run by digest

```console
# submitter
$ spfs env resolve /show/abc/tst/tst0100/lgt --published-only --json
{"digest": "R8YWK...====", "sources": [...], "solve": {...}}
$ spfs tag R8YWK...==== jobs/8842107          # retention tag, pruned per policy
$ submit --env-digest R8YWK...==== ...

# each of 5,000 workers
$ spfs run R8YWK...==== -- houdini -j "$FRAME_ARGS"
```

Workers never probe, never crawl, never solve. Job start touches the repo only to sync missing objects, and per-process-tree runtimes mean concurrent jobs on one host are fully isolated from each other with `spfs-monitor` cleaning up behind each. `--published-only` (implied whenever stdin isn't a tty) hard-disables slots 3 and 4, so a stray override file in a show directory cannot leak into farm renders; if the submitter's interactive resolution used a bind mount, submission **fails** with "environment is not reproducible" rather than warning.

> ⚠ **Problem: tags move mid-show, jobs run for 12 hours.** **Answer:** already solved by the rule. The digest was fixed at submit; a publish an hour later changes *new* submissions only. For "everything in tonight's dailies used one environment," resolve once per batch (optionally `--as-of @2026-07-29T18:00:00-07:00`) and stamp that digest on the whole batch.

> ⚠ **Problem: 5,000 workers sync at once.** The burst hits at frame-0. **Answer:** layered, all existing spfs machinery: per-site proxy/fallback repos so workers pull from close storage; pool pre-pull keyed off the `jobs/*` tags as they appear; and because show stacks share their bottom flatten groups, the marginal sync for "one more job on abc" is usually near zero. The number to watch is `spfs server` connection concurrency at frame-0; measure before trusting.

### 8.2 Long-running services with per-show environments

A review daemon, a publish validator, a bot that runs shots' `pytest` — services that must execute *inside* each show's environment without restarting per show. The pattern is spawn-per-task:

```python
digest = resolve_cached(show_path)             # spfs env resolve --json, cached w/ TTL
subprocess.run(["spfs", "run", digest, "--", "review-worker", "--task", task_id])
```

The service itself stays outside any runtime; each task gets a clean, isolated, digest-pinned environment that evaporates on exit. Cache resolutions keyed on the digest the resolver returns (it already encodes file hashes + resolved targets), with a short TTL as the staleness bound for moving tags.

> ⚠ **Problem: nested runtimes are unverified.** A worker spawned inside a runtime asking for another runtime is exactly the migration-era shape (§9 of the design doc flagged it). **Answer this guide can't give:** test it in week one. The service pattern above deliberately keeps the daemon *outside* any runtime so the question stays academic for services; for migration it must actually be answered.

---

## 9. Debugging

Ordered by how often you'll reach for each.

### 9.1 Where am I? — `spfs env status`

```console
[abc/comp] $ spfs env status
platform  LQ7XN...====                     resolved 2026-07-29 14:02 by apaxson
sources   ilm/base                         J3K9...  (slot 1)
          ilm/show                         K2MM...  (slot 2)
          ilm/show/abc                     X2PQ...  (slot 2)
          ilm/show/abc/tst/tst0100         P0DD...  (slot 2)
          /show/abc/tst/.../.spfs-env.yml  sha256:88a1…  (slot 3)  [override]
          ~/.spfs-env.yml                  sha256:1f0c…  (slot 4)
solve     34 packages (spk)                spfs env explain --solve
warnings  none · reproducible · lock clean
```

The `[override]` marker alone answers "is my environment stock?" — the first question in every support thread.

### 9.2 Why is X here? — `spfs env why`

Provenance for all three composition rules, because "why" means something different under each:

```console
$ spfs env why /spfs/bin/nuke              # Rule 1: which layer provides this file,
provided by  ilm/app/nuke/15.1  (via requests: nuke/15.1 @ ilm/show/abc)
shadowed     ilm/app/nuke/15.0  (1 layer below — masked? no, shadowed)

$ spfs env why --var PYTHONPATH            # Rule 3: the composition trace, in order
 1 prepend /spfs/ilm/lib/python        ilm/tools/fxutils/1.4.0 (self-describing)
 2 prepend /spfs/show/abc/python       ilm/show/abc
 3 prepend /spfs/user/apaxson/py       ~/.spfs-env.yml   [override]
 = /spfs/user/apaxson/py:/spfs/show/abc/python:/spfs/ilm/lib/python

$ spfs env why --package mtoa              # Rule 2: request accumulation + the solve
requested  mtoa/5.4  @ ilm/show/abc (apps.maya)
replaced   ←  mtoa/5.3  @ shot override file (replaces: [mtoa])
solved     mtoa/5.3.2  (spk; constraints: maya/2024.2 → mtoa<5.4 ✓)
```

File-level `why` is spfs's existing `find_path` machinery surfaced with slot labels; the other two are resolver data. The mask gotcha gets its own hint: querying a masked path prints *which level masked it* and reminds you masks only hide what's **below** them.

### 9.3 What changed? — `spfs env diff` and the lock flow

```console
$ spfs env
! environment changed since you last used this path
  was  A3BCX...====   (locked 2026-07-22)
  now  9KKDM...====
  why  ilm/show/abc moved (published 2026-07-28 — spfs log ilm/show/abc)
  see  spfs env diff --lock        accept  spfs env --reapply
  or pin yesterday exactly:        spfs env --digest A3BCX...====
```

The lock is trust-on-first-use per path, stored under the *user's* home (a lock in the show dir could be rewritten by whoever rewrote the spec). `diff` composes three views — layer-level (stack delta), file-level (`spfs diff`, existing), env/config-level (composition delta) — and takes any two of: paths, digests, `--as-of` dates, `--lock`, `--mine` (with vs without slots 3+4, the "works on my machine" scalpel).

### 9.4 Reproduce yesterday / reproduce the farm

```console
$ spfs env --digest R8YWK...====           # from a lock, a job payload, or a ticket
```

Plus the two time machines, which answer different questions: `--as-of @2026-07-01 <path>` = *what did the published environment say then* (tags only, overrides excluded by construction — mixing July's tags with today's files answers nothing); `--digest` = *run exactly what ran*, overrides included. The runtime's invocation annotations (`ilm.inv.*`: source list + hashes, resolver version, live-layer flag — kept **off** the platform so digests stay stable) make every runtime carry its own black-box recorder, and a farm ticket needs exactly one string.

The honest failure: if the original used a bind mount, reproduction says so — *"included live bind of /home/x/work; contents at that time unrecoverable; nearest committed state: user/x/mytool/dev (promoted 2026-07-28)"* — instead of silently handing back something different. That sentence is the difference between a trusted tool and a routed-around one.

### 9.5 `spfs env doctor`, and the failure table

`doctor` checks the environment *around* the tool: repo reachability per remote, allowlist coverage of the canonicalized cwd (automount/symlink divergence is caught here), encoding format (Legacy blocks annotations → everything in §4 breaks), stack depth + flatten-reuse stats, render-cache warmth, oversized config values, clock skew vs repo (bites `--as-of`).

| Symptom | First move | Usual cause |
| --- | --- | --- |
| `unknown reference ilm/show/abc/...` | `spfs env explain` | A gap level (legal, gaps skip) vs a typo'd path (error) — explain shows which |
| Solve failure entering a shot | `spfs env why --package X` | Two levels' requests intersect to ∅ → add `replaces` at the deeper level |
| File present but wrong version | `spfs env why <path>` | Shadowing (Rule 1, silent) — the fix usually belongs in Rule 2 |
| Env var wrong inside DCC | `spfs env why --var V` | A `startup.d` script fighting the resolver (alphabetical-order trap) — move it to `env:` |
| Works locally, fails on farm | `spfs env diff --mine` | Slot 3/4 content, or a bind mount the submit should have refused |
| First launch very slow | `spfs env doctor` | Cold render cache (one-time) — pre-warm pools |
| Digest changed, nobody changed *my* files | lock banner → `spfs log <level>` | An ancestor level published — correct propagation, see who/when in the stream |
| `--digest` fails: unknown object | — | Aged past GC retention (§3.5) — tooling names the date and nearest survivor |

---

## 10. Appendix

### What exists today vs what this guide invents

| Exists in spfs/spk now | Proposed (`spfs env` + resolver) |
| --- | --- |
| Platforms, stacks, digests, tags + streams, `spfs log/diff/info/run/tag/pull/push/clean` | Path→tag probing, upward crawl, slots, interleaving |
| Annotations (incl. spill-to-blob; spk stores solve data in one) | Merged-config read path, schema governance, `config get` |
| Live layers (bind), mapping-layer building blocks (public Manifest APIs), masks, flattening | `promote`, self-describing layers, publish pipeline, app profiles |
| `?when=@date` pinned repos, `--environment-override` on spfs-enter | Lock files + alert flow, `explain`/`why`/`diff --mine`/`doctor` |
| External-subcommand dispatch (ships `spfs env` with zero spfs changes) | Everything else in this guide |

### Design-doc decisions this guide took a position on

- **Decision 1** → versioned prefixes for DCCs/tools (§6.1); `mask` demoted to escape hatch.
- **Decision 3** → depth-interleaved slots 2/3, restated throughout.
- **Decision 4** → solved spk layers sit low; explicit layers/overrides win (§2 stack, §6.2).
- New here, to upstream into the design doc: **no-default `api:`** (§2), **merged-config annotation + blessed read path** (§4.2), **app profiles** (§6.2), **self-describing layers** (§5.2), **`jobs/*` retention tags + GC honesty** (§3.5, §9.4), **`--published-only` implied when not a tty** (§8.1).

### Known upstream sharp edges this guide routes around

annotation reader precedence disagreement (untested upstream — the merged-config path exists because of it) · `add_annotation` in a loop = one layer per key · Legacy encoding blocks annotations entirely · `startup.d` alphabetical ordering vs layer order · spk's `setup_runtime` **replaces** `rt.status.stack` (resolver must own the solve) · nested-runtime behavior unverified · docs bugs list in the design doc appendix.
