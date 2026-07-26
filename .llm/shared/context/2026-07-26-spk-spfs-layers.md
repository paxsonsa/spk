---
date: 2026-07-26T20:52:00Z
repository: spk
git_commit: 47c3bfd2d9f6dc6118ed828abadb52c6d28243ec
branch: worktree-spfs-discovery-doc
discovery_prompt: "how does the spfs layer system and file work and what does it support?"
generated_by: "opencode:/discovery"
tags: [context, discovery, spfs, layers]
status: complete
last_updated: 2026-07-26
---

# Repo Context Guide: SPFS layers and layer files

Focused companion to [`2026-07-26-spk-spfs.md`](./2026-07-26-spk-spfs.md), which covers SPFS broadly. This one goes deep on one question: what a layer actually is, how a stack of them becomes the `/spfs` you see, and what the two YAML "layer files" do.

## TL;DR

- A **Layer** is a tiny object: an optional manifest digest plus an optional list of annotations. That's the whole thing. It holds no file data itself — the manifest it points at does.
- Because the manifest field is optional, three shapes of layer are legal: manifest-only (the normal case), annotation-only (metadata with no files), and both.
- A **Stack** is an ordered, deduplicating list of digests, bottom-up. Pushing a digest that's already present *moves* it to the top rather than duplicating it.
- Stacks resolve recursively: a platform in a stack expands into its own layers, and a bare manifest digest gets wrapped into a synthetic layer on the fly.
- Merge rule is last-wins, bottom-up. `Manifest::update` walks the incoming tree and overwrites entries; a `Mask` entry deletes the name and then reinserts itself as a tombstone so lower layers stay hidden.
- Two different YAML files both end in `.spfs.yaml` and are told apart by their `api:` field: `spfs/v0/runspec` (a list of refs, purely a command-line shorthand) and `spfs/v0/livelayer` (bind mounts of host paths into `/spfs`, which never become spfs objects at all).
- The overlayfs backend has a hard ceiling: mount options must fit in one page. When the stack is too tall, spfs **flattens** groups of 7 layers into new merged manifests to fit. The FUSE backend has no such limit because it merges everything into one manifest up front.

## How a layer is defined

`crates/spfs-proto/schema/spfs.fbs`:

```
table Layer {
    manifest:Digest;                    // optional
    annotations:[Annotation] (required); // may be empty
}
```

`manifest` is not marked required — a layer with no manifest is valid and is how `spfs run --annotation` works. `annotations` is required as a *vector*, but the vector may be empty.

The Rust side (`crates/spfs/src/graph/layer.rs`) is `FlatObject<spfs_proto::Layer>` with a builder:

```rust
Layer::new(manifest_digest)
Layer::new_with_annotation(key, value)
Layer::new_with_annotations(pairs)
Layer::new_with_manifest_and_annotation(digest, key, value)
Layer::builder().with_manifest(d).with_annotations(v).build()
```

`Layer::child_objects()` returns the manifest digest (if any) plus any blob digests referenced by spilled annotation values. That's what the GC and sync traversals follow.

### Two gotchas worth internalizing

**1. `PartialEq` and `Hash` ignore annotations.**

```rust
impl std::hash::Hash for Layer {
    fn hash<H>(&self, state: &mut H) { self.proto().manifest().hash(state) }
}
impl std::cmp::PartialEq for Layer {
    fn eq(&self, other: &Self) -> bool { self.proto().manifest() == other.proto().manifest() }
}
```

Two layers with the same manifest but different annotations are `==` and hash the same, **yet have different digests** (the digest computation in `graph/object.rs` includes annotations regardless of encoding format). This is deliberate — `resolve_stack_to_layers_with_repo` ends with `.unique()`, and for mounting purposes two layers with the same manifest really are the same rendered directory. But it means all annotation-only layers (manifest `None`) collapse to a single entry under `.unique()`. Harmless today because annotation-only layers contribute no directories to the mount, but do not reuse `Layer` as a set key expecting annotation identity.

**2. Legacy encoding cannot store annotations.**

`Layer::legacy_encode` hard-errors if annotations are present. `Layer::digest_encode` *does* include them regardless of format, so digests stay stable across the format boundary. The practical consequence surfaces at the CLI: `spfs run --annotation` refuses to run when `storage.encoding_format = "Legacy"` (`crates/spfs-cli/main/src/cmd_run.rs`).

## How a stack works

`crates/spfs/src/graph/stack.rs`. A `Stack` is a hand-rolled singly linked list of digests, bottom at the head.

```rust
pub fn push(&mut self, digest: Digest) -> bool
```

Push walks the whole list, **removing any existing node with the same digest**, then appends at the top. Returns `false` only when the digest was already the topmost entry (no change). So a stack is an ordered *set*, and re-pushing an existing layer promotes it rather than duplicating it. `Committer::commit_manifest` relies on the `false` return to raise `Error::NothingToCommit`.

Iteration is `iter_bottom_up()` (cheap, lazy) or `to_top_down()` (allocates and reverses — the doc comment says prefer bottom-up).

**Serialization is reversed.** For backward compatibility, stacks serialize top-down and deserialize by reversing:

```rust
// Serialize
serializer.collect_seq(self.to_top_down())
// Deserialize
Ok(Self::from_iter(s.into_iter().rev()))
```

So the JSON you see in `spfs runtime info` lists the top layer first, while all the in-memory iteration is bottom-first. Easy to get backwards when writing tooling against the stored form.

### Resolving a stack to layers

`resolve_stack_to_layers_with_repo` (`crates/spfs/src/resolve.rs`) walks the stack bottom-up and, per digest:

| Object kind found | What happens |
| --- | --- |
| `Layer` | Pushed as-is |
| `Platform` | **Recursively expanded** into its own stack's layers, spliced in place |
| `Manifest` | Wrapped in a synthetic `Layer::new(manifest_digest)` — you can put a bare manifest digest in a stack |
| `Blob` | Error: "Cannot resolve object into a mountable filesystem layer" |

The result is deduplicated with `.unique()` before returning, because overlayfs fails outright if the same directory appears twice in the lowerdir list.

Platform expansion is recursive with no depth guard (`#[async_recursion]`), so a platform containing a platform containing a platform flattens fine, but there's nothing stopping a pathological nesting depth.

## How layers merge into a filesystem

Two distinct mechanisms, depending on backend.

### Manifest merging (used for masking, FUSE, and `spfs info`)

`tracking::Manifest::update(&other)` layers `other` on top of `self`. The real work is `Entry::update` (`crates/spfs/src/tracking/entry.rs`):

```rust
pub fn update(&mut self, other: &Self) {
    self.kind = other.kind;
    self.object = other.object;
    self.mode = other.mode;
    if !self.kind.is_tree() { return; }

    for (name, node) in other.entries.iter() {
        if node.kind.is_mask() {
            self.entries.remove(name);
        }
        if let Some(existing) = self.entries.get_mut(name) {
            existing.update(node);
        } else {
            self.entries.insert(name.clone(), node.clone());
        }
    }
}
```

Read that carefully — the mask branch removes the entry and then, because `get_mut` now returns `None`, **inserts the mask node itself**. The tombstone propagates into the merged manifest. That's intentional: `mask_files` later walks the merged manifest looking for exactly those `Mask` entries.

Also note `self.kind = other.kind` is unconditional and the early return checks the *new* kind. So a file in an upper layer replacing a directory in a lower one silently drops the whole subtree, which is the correct filesystem semantic.

Callers apply this bottom-up, so **higher layers win**:

```rust
// resolve.rs :: compute_environment_manifest
let layers = resolve_stack_to_layers(&stack, Some(repo)).await?;
for layer in layers {
    if let Some(d) = layer.manifest() {
        manifest.update(&repo.read_manifest(*d).await?.to_tracking_manifest())
    }
}
```

### `EntryKind` and deletion

```rust
pub enum EntryKind {
    Tree,        // directory
    Blob(u64),   // file, with size
    Mask,        // removed entry
}
```

`Mask` is how deletion is represented in the content-addressed world — you cannot mutate a lower layer, so an upper layer records a tombstone. On the overlayfs side this gets translated into a real whiteout: `mask_files` in `env.rs` walks the merged manifest and for each `Mask` node calls `mknod(path, S_IFCHR, 0)` in the upper dir. `runtime::is_removed_entry` recognizes those on the way back in (character device with `rdev() == 0`, the overlayfs whiteout convention).

### Overlayfs stacking (`OverlayFsWithRenders`, the Linux default)

Each layer's manifest is **rendered** to a real directory on disk under `<storage.root>/renders/<user>/`, hard-linked from `payloads/` so the bytes are not duplicated (only inodes). Render strategies live in `storage/fs/renderer.rs`:

| `CliRenderType` | Behavior |
| --- | --- |
| `HardLink` | Default. Hard-link via a proxy directory for cross-user dedup |
| `HardLinkNoProxy` | Hard-link directly, no proxy |
| `Copy` | Full copy. **Forced for durable runtimes** — they mount without overlayfs `index=on`, and hardlinks would cause edit aliasing |

Then `get_overlay_args` (`env.rs`) builds the mount string. Two details that bite:

- Lowerdirs are emitted **in reverse**, because overlayfs reads right-to-left (rightmost = bottom). The runtime's own `lower_dir` is always appended last as the true bottom, which is why an empty runtime still has one layer for overlayfs to work with.
- On Linux ≥ 6.8 spfs uses the `lowerdir+=` append syntax; otherwise the older `lowerdir=a:b:c` form. Detected by parsing `modinfo overlay` output (`runtime/overlayfs.rs`).

### Automatic layer flattening

The mount option string must fit within one page (`sysconf(PAGE_SIZE) - 1`). Deep stacks blow past that. `resolve_overlay_dirs` handles it by merging layers:

```
loop {
    build the lowerdir list
    if get_overlay_args(...) fits → done
    otherwise: halve the layer count by merging groups of 7 from the bottom
}
```

The group size is a fixed `FLATTEN_GROUP_SIZE: usize = 7`, chosen so flattened groups are **reusable across runtimes** — two environments sharing the same bottom 7 layers produce the same flattened manifest digest, so the render can be reused. The code comment illustrates it:

```
A B C D E F G H I J K L M N O P ...
|---- A' -----|---- H' -----|

A B C D E F G H I J K L Q R S T ...
|---- A' -----|---- H'' ----|
```

Merged manifests are written to the repo and their digests recorded in `runtime.status.flattened_layers` — a `HashSet<Digest>` separate from the stack. Its only job is to keep a strong reference so cleaning doesn't collect them while a runtime is live. `Runtime::to_platform()` folds them back in:

```rust
pub fn to_platform(&self) -> graph::Platform {
    let mut stack = self.status.stack.clone();
    stack.extend(self.status.flattened_layers.iter());
    stack.into()
}
```

This is worth knowing when debugging: the platform stored for a runtime can contain manifests that were never explicitly requested, and `flattened_layers` is a set, so their relative order is not meaningful.

Flattening is a "proposed" operation — `ResolvedManifest::Proposed` holds a `NonEmpty` tree of candidates, and nothing is written to the repo until the loop settles. A proposed group can be merged into a larger group on a later iteration, so intermediate merges never cost a write.

### FUSE stacking

No per-layer directories, no flattening, no arg limit. `spfs-fuse` calls `compute_environment_manifest` once at startup, merging the entire `EnvSpec` into a single `tracking::Manifest`, pre-allocates inodes for every entry, and serves from that (`crates/spfs-vfs/src/fuse.rs`). Layers stop existing as separate things the moment the filesystem starts.

## The two layer files

Both are YAML, both must be given as an **absolute path** in an EnvSpec, both must end in `.spfs.yaml` (or be a directory containing `layer.spfs.yaml`). They are distinguished only by the `api:` field.

`crates/spfs/src/runtime/spec_api_version.rs`:

```rust
pub enum SpecApiVersion {
    #[serde(rename = "spfs/v0/livelayer", alias = "v0/livelayer", alias = "v0/layer")]
    V0Layer,        // <- the Default
    #[serde(rename = "spfs/v0/runspec")]
    V0EnvLayerList,
}
```

Note `V0Layer` is `Default`, so a spec file with **no** `api:` field parses as a live layer.

### 1. Run spec — `api: spfs/v0/runspec`

A list of refs, to keep command lines short. Purely a parse-time expansion; it produces no spfs object.

```yaml
api: spfs/v0/runspec
layers:
  - A7USTIBXPXHMD5CYEIIOBMFLM3X77ESVR3WAUXQ7XQQGTHKH7DMQ====
  - spfs/some/tag/to/something
  - /abs/path/to/another.spfs.yaml     # nested — flattened recursively
```

```sh
spfs run /path/to/runspec.spfs.yaml
spfs run SOMEDIGEST+/path/to/runspec.spfs.yaml+SOMETAG
```

`EnvLayersFile::flatten()` recursively expands nested run specs inline. `parse_env_spec_items` splices the result straight into the item list, so by the time anything downstream sees an `EnvSpec`, run-spec files have vanished.

**Recursion guard**: a process-global `SEEN_SPEC_FILES: Mutex<HashSet<PathBuf>>` records every spec file path parsed. A repeat raises `Error::DuplicateSpecFileReference`, and `parse_env_spec_item` special-cases that error so it propagates instead of falling through to "maybe it's a tag." The cache is process-global and only cleared by explicitly calling `tracking::clear_seen_spec_file_cache()` — relevant for long-lived processes and tests that parse the same file twice.

### 2. Live layer — `api: spfs/v0/livelayer`

Bind mounts of host paths into `/spfs`. Files stay live and writable on the host — this is the escape hatch for putting a git checkout inside `/spfs` during development.

```yaml
api: spfs/v0/livelayer
contents:
  - bind: docs/use          # 'bind' or 'src'
    dest: /spfs/docs        # /spfs prefix optional
  - bind: tests/some.data
    dest: test_data/some.data
```

Rules enforced by `LiveLayer::set_parent_and_validate` (`runtime/live_layer.rs`):

- Every `src` is resolved **relative to the directory containing the YAML file**, then canonicalized.
- Every resolved `src` must still be **under** that parent directory. You cannot bind-mount `/etc` from a live layer sitting in your home directory.
- Every `src` must exist at parse time.
- Only directories and regular files are supported; anything else errors.

Live layers are **not spfs objects**. They have no digest, so:

- `EnvSpecItem::resolve_digest` on one returns `"Impossible operation: spfs env files do not have digests"`.
- `with_tag_items_resolved_to_digest_items` filters them out entirely before syncing.
- `compute_environment_manifest` skips `SpecFile` items when building the stack.
- They live in `runtime.config.live_layers`, persisted with the runtime, not in the stack.

**The mount-point trick.** Bind mounts need their destinations to exist inside `/spfs` first. `Runtime::ensure_extra_bind_mount_locations_exist` builds a temp directory containing just the destination paths (empty dirs, empty files), computes a manifest from it, commits that as a **real layer**, and pushes it onto the stack. So using a live layer does create one genuine spfs layer — a skeleton of empty mount points.

That function also enforces ordering: bind mounts are applied in list order, so a directory mount can shadow a later file mount whose destination lives inside it. Rather than silently losing the file, spfs checks whether the directory's source already contains a file of that name and errors with "Invalid extra mount order: … please reorder these extra mounts" if not.

## Annotations: layers without files

`spfs run --annotation key=value` builds a layer with no manifest and pushes it on the stack (`Runtime::add_annotation`). Values are stored inline as strings when small, or spilled to a blob when large:

```rust
let annotation_value = if value.len() <= size_limit {
    AnnotationValue::string(value)
} else {
    AnnotationValue::blob(self.storage.create_blob_for_string(value).await?)
};
```

The threshold is `filesystem.annotation_size_limit`, defaulting to `DEFAULT_SPFS_ANNOTATION_LAYER_MAX_STRING_VALUE_SIZE = 16 * 1024` (`graph/annotation.rs`).

Lookup (`Storage::find_annotation`) walks the stack **bottom-up**, expanding platforms as it goes, and returns the first match. `cmd_run.rs` adds annotations in **reverse** command-line order specifically so that later `--annotation` flags land lower in the stack and therefore win the lookup. That inversion is easy to trip over: for annotations, bottom-up-first means *earlier in the stack wins*, which is the opposite of how file layers resolve.

`all_annotations()` collects every key from the whole stack into a `BTreeMap`, so later entries overwrite earlier ones — the opposite precedence from `annotation()`. Two accessors, two orderings.

Read them with `spfs info --get <KEY>` / `--get-all`, or `spfs runtime info --get-all`.

## What the layer system supports

| Capability | How | Where |
| --- | --- | --- |
| Content-addressed immutable layers | SHA-256 over the encoded object | `graph/object.rs` |
| Ordered stacking with last-wins override | `Stack` + `Manifest::update` | `graph/stack.rs`, `tracking/entry.rs` |
| Automatic dedup in a stack | `Stack::push` removes-then-appends | `graph/stack.rs` |
| Nested composition | Platforms expand recursively into layers | `resolve.rs` |
| Bare manifests as layers | Auto-wrapped in a synthetic layer | `resolve.rs` |
| Deletion across layers | `EntryKind::Mask` → overlayfs whiteout `mknod` | `env.rs`, `runtime/overlayfs.rs` |
| Metadata-only layers | Layer with `manifest: None` + annotations | `graph/layer.rs` |
| Large metadata values | Spilled to a blob, referenced by digest | `graph/annotation.rs` |
| Deep stacks beyond kernel limits | Group-of-7 flattening with reusable digests | `resolve.rs` |
| Host paths inside `/spfs` | Live layers (bind mounts) | `runtime/live_layer.rs` |
| Ref lists in a file | Run spec files, recursively flattened | `tracking/env.rs` |
| Tag history as a ref | `tag~N` in an EnvSpec | `tracking/tag.rs` |
| Point-in-time views | `?when=` on a remote, pinned repos | `storage/pinned/` |
| "Which layer provides this file?" | `find_path_providers_in_spfs_runtime` | `find_path.rs` |
| Cross-user render dedup | Hard links via `renders/<user>/proxy` | `storage/fs/renderer.rs` |

### What it does not support

- **Modifying a committed layer.** Everything is content-addressed; you make a new layer and move a tag, exactly like a git branch.
- **Ordering guarantees for flattened layers.** `flattened_layers` is a `HashSet`, so the order they get folded into `to_platform()` is arbitrary. They exist for reference-keeping, not for stacking.
- **Live layers in remote storage.** They are host-local bind mounts with no digest; they cannot be pushed, pulled, or shared. Only the empty mount-point skeleton layer is a real object.
- **Annotations under legacy encoding.** `legacy_encode` errors, and the CLI blocks it up front.
- **Arbitrary bind mount sources.** Live layer sources must be under the spec file's own directory.
- **Unbounded stack depth on overlayfs.** It works, but past a certain depth you silently start paying for flatten-and-render instead of reusing existing renders.
- **Symlink or device bind mounts in live layers.** Directories and regular files only.

## LLM working set

1. `crates/spfs-proto/schema/spfs.fbs` — the layer table, 4 lines that define everything
2. `crates/spfs/src/graph/layer.rs` — builder, the Eq/Hash quirk, legacy encoding limits
3. `crates/spfs/src/graph/stack.rs` — push semantics and the reversed serialization
4. `crates/spfs/src/resolve.rs` — stack→layers→manifests→rendered dirs, plus flattening
5. `crates/spfs/src/tracking/entry.rs` — `Entry::update`, the actual merge rule
6. `crates/spfs/src/tracking/env.rs` — EnvSpec parsing, both spec file kinds, recursion guard
7. `crates/spfs/src/runtime/live_layer.rs` — bind mounts and their validation
8. `crates/spfs/src/runtime/spec_api_version.rs` — the `api:` discriminator
9. `crates/spfs/src/env.rs` — `get_overlay_args`, `mask_files`
10. `crates/spfs/src/commit.rs` — how new layers and platforms get created

### Q&A anchors

- *"Why did my layer not get added?"* → `Stack::push` returns `false` if it was already on top; `commit_manifest` turns that into `NothingToCommit`.
- *"Why does the stored stack look reversed?"* → it is. Serialization is top-down for backward compatibility; in-memory iteration is bottom-up.
- *"Why are there layers in my platform I never asked for?"* → `flattened_layers`, folded in by `to_platform()`.
- *"Why is my `--annotation` being ignored?"* → later flags are pushed lower deliberately; `annotation()` returns the first hit walking bottom-up. Check whether you also set `encoding_format = "Legacy"`, which blocks annotations entirely.
- *"Why won't my live layer bind mount?"* → source must be under the spec file's directory, must exist, and must not be shadowed by an earlier directory mount.
- *"How do I find which layer provides a file?"* → `find_path_providers_in_spfs_runtime` (`find_path.rs`), surfaced through `spfs info`.

### Glossary

| Term | Meaning |
| --- | --- |
| **Layer** | Optional manifest digest + optional annotations. Holds no file data itself |
| **Manifest** | The actual filesystem tree a layer points at |
| **Stack** | Ordered dedup'd list of digests, bottom-up, top wins |
| **Platform** | A persisted stack; expands recursively when resolved |
| **Mask** | Tombstone entry marking a deletion relative to lower layers |
| **Whiteout** | The overlayfs on-disk form of a mask: char device, rdev 0 |
| **Render** | A manifest materialized as a real directory, hard-linked from payloads |
| **Flattened layer** | A synthetic merged manifest created to fit the overlayfs arg limit |
| **Live layer** | Host paths bind-mounted into `/spfs`; not an spfs object |
| **Run spec** | A YAML list of refs, expanded at parse time |
| **Annotation** | Key/value data in a layer, readable from inside the runtime |
| **EnvSpec** | `+`-separated list of refs; the thing you pass to `spfs run` |

## Doc/code notes

`docs/spfs/usage.md` covers both file formats accurately, including the `api:` discriminator, the `bind`/`src` alias, and the mount-point-creation behavior. Two small things it leaves out:

1. It doesn't mention that `api:` **defaults to livelayer** when absent, so a run spec missing its `api:` line will fail to parse as a live layer rather than erroring clearly about the missing field.
2. It doesn't mention the duplicate-spec-file guard, so a run spec that references itself (directly or through a nest) produces `DuplicateSpecFileReference` rather than a stack overflow. Good behavior, undocumented.

Nothing in `docs/spfs/` describes automatic layer flattening or `flattened_layers`, which is the most surprising behavior in the whole layer system — deep stacks silently get restructured.

## Open questions

- **Flatten group size.** `FLATTEN_GROUP_SIZE = 7` is a magic constant with a good rationale comment but no benchmark cited. Whether 7 is tuned for real stack depths at ILM scale is unclear. Check `crates/spfs/benches/spfs_bench.rs` and `crates/spfs/src/resolve_test.rs` for coverage.
- **Platform recursion depth.** `resolve_stack_to_layers_with_repo` is `#[async_recursion]` with no cycle detection. A platform that (through storage corruption or a hand-crafted object) references itself would recurse forever. Worth confirming whether anything upstream prevents constructing that.
- **`flattened_layers` lifecycle.** It's only updated when the computed set differs, and cleared implicitly by being overwritten. Whether stale flattened manifests from a previous run of a durable runtime get collected is not obvious — check `Cleaner` against `runtime.to_platform()`.
- **Annotation precedence inversion is untested.** `annotation()` (first match walking bottom-up → lowest wins) and `all_annotations()` (BTreeMap overwrite while walking bottom-up → highest wins) disagree on which value wins when the same key appears in two layers. The five annotation tests in `crates/spfs/src/runtime/storage_test.rs` all use distinct keys, so nothing pins this down. Since `cmd_run.rs` reverses the CLI order specifically to make `annotation()` behave, the inversion looks incidental rather than designed — worth a decision and a test before anyone depends on either.
