# SPEED.md — build/iteration speedup notes

Iteration time for `npx tauri build --debug --features webdriver` (the verify-via-MCP loop documented in CLAUDE.md) is 5–15 min per cycle. This doc explains where the time goes and lists fixes in priority order.

All measurements taken on 2026-05-31 / 2026-06-01 on this machine (Apple Silicon, rustc 1.93.0 via Homebrew, macOS Darwin 25.5.0).

## Measured results (2026-06-01)

Ran a head-to-head test with the same starting state (touch `src-tauri/src/main.rs`):

| Approach | Wall time | Notes |
|---|---|---|
| `npx tauri build --debug --features webdriver` (the CLAUDE.md command) | **16m 26s** (986 s) | Includes blocking on `tauri dev`'s lock, full feature recompile, and DMG bundling. |
| `cargo build --bin arimalo-covid --features "webdriver tauri/custom-protocol"` (cache still warm from the run above) | **2m 13s** (134 s) | Skips 11 CLI bins and bundler. |
| Same command, true warm cache (touch + rebuild) | **8.27 s** | The realistic inner-loop incremental rebuild. |

Speedup: **7.4×** on the side-by-side run, and **~120×** for a true warm-cache incremental rebuild. Per-edit iteration went from ~16 min to **~8 s** — fast enough to keep flow state.

The resulting binary at `src-tauri/target/debug/arimalo-covid` is a 76 MB Mach-O executable that runs directly; no `.app` wrapper needed for MCP/tauri-wd.

### Secondary finding: workflow-switching cost is asymmetric

The 16-min build came partly from blocking on a `tauri dev` lock and partly from genuine cold compilation of the webdriver dep tree. Confirmed by a follow-up test on 2026-06-01:

- **`tauri dev` → `tauri build --features webdriver` (heavy direction).** The `webdriver` Cargo feature pulls in `tauri-plugin-webdriver-automation` + its transitive deps, which were not in the previous compile graph. That's genuinely new compile work: **5–15 min** on a cache that's cold-for-webdriver. The original 16-min baseline was this direction.
- **`tauri build --features webdriver` → `tauri dev` (light direction).** Measured **47 s** from `scripts/run_debug.sh` invocation to running app, of which only ~20–30 s was cargo. Cargo drops the webdriver plugin from the link, recompiles the arimalo-covid CGUs (different cfg flags), and relinks; the heavy deps stay cached.

Implication: `run_debug.sh` after an MCP-verify build is cheap and you can ignore it. The expensive trip is the other way — that's the one `scripts/run_debug_mcp.sh` (isolated `CARGO_TARGET_DIR`) is for. Also: always kill stale `tauri dev` before any new build — file-lock blocking can add a couple of minutes of pure waiting on top of whatever compile work follows.

## TL;DR — the highest-leverage fixes (priority order)

1. **Stop building 12 binaries when you only need one.** `tauri build` invokes `cargo build --bins` which compiles every `[[bin]]` in `Cargo.toml` (arimalo-covid + 11 CLIs). Switch to `cargo build --bin arimalo-covid --features "webdriver tauri/custom-protocol"` (with `npm run build` first) — confirmed below to drop the iteration from ~16 min to ~8 s on warm cache.
2. **Stop building a DMG on every iteration.** `tauri build --debug` runs the full bundler — `target/debug/bundle/dmg/Arimalo COVID_0.1.0_aarch64.dmg` (57 MB) is regenerated each build. On macOS this is `hdiutil` + compression + cleanup, typically 60–120 s per cycle. Use `--no-bundle` or just `cargo build` for inner-loop work. (Bonus: covered by fix #1.)
3. **Workflow-switching cost is asymmetric.** `tauri dev` → `tauri build --features webdriver` is the expensive direction (5–15 min cold; the `webdriver` feature pulls in `tauri-plugin-webdriver-automation` + transitive deps that weren't in the previous graph). The reverse trip is ~30–60 s of relink and you can ignore it. `scripts/run_debug_mcp.sh` (isolated `CARGO_TARGET_DIR`) exists for the heavy direction.
4. **Don't run concurrent cargo builds against the same `target/` dir.** Cargo serialises behind a global lock. If you have multiple agents/terminals iterating in parallel, give them separate `CARGO_TARGET_DIR`s.
5. *(Skip for now)* Linker swap. The macOS story is worse than I expected: `mold` no longer supports Mach-O, Homebrew's `llvm` no longer ships `lld`, and Apple's stock `ld-prime` is already fast enough that the inner-loop case is dominated by rustc codegen. Details in section 2.

Adopting (1) alone takes the inner loop from 16 min to ~8 s on warm cache. **Verified by direct measurement** — see "Measured results" above.

## What the build actually does today

The `npx tauri build --debug --features webdriver` command runs:

```
npm run build                                        # vite — ~3-5 s
cargo build --bins --features webdriver,tauri/custom-protocol   # the big one
tauri bundler: create .app, hdiutil DMG, codesign    # 60-120 s on macOS
```

Confirmed by inspecting the in-flight process:

```
501 58998  cargo build --bins --features webdriver,tauri/custom-protocol
```

That `--bins` is the giveaway — Tauri 2's CLI passes it because the package has 12 `[[bin]]` entries and no other hint about which is "the app". Cargo dutifully links all of them.

### Compile graph size

- `cargo tree` reports **542 unique transitive deps**, 1,215 lines.
- Heaviest rlibs in `target/debug/deps/`:
  - `libobjc2_app_kit` — 140 MB (×3 copies)
  - `libobjc2_foundation` — 72 MB (×3 copies)
  - `libautomerge` — 64 MB (×3 copies)
  - `librhai` — 50 MB
- Multiple copies exist because the build matrix has shifted (feature flags, host vs build, dev vs release-profile of build scripts).

### Project code

- 27,819 lines of Rust across 45 files in `src-tauri/src/`.
- `main.rs` alone is **3,462 lines** — every edit to `main.rs` invalidates the entire `arimalo-covid` binary CGUs.
- `lib.rs` re-exports 22 sibling modules. Touching `lib.rs` (or any module) invalidates **all 12 CLI binaries** + the main app.
- Frontend: 54 TypeScript files. Vite build is fast (single-digit seconds) and not the bottleneck.

### Disk state

- `src-tauri/target/` is **83 GB**, of which **75 GB is `debug/`**.
  - `debug/deps/` — 63 GB
  - `debug/incremental/` — 9 GB
  - `debug/build/` — 2.6 GB (520 build-script output dirs; many duplicates from old hashes)
  - `debug/bundle/` — 549 MB, including two orphaned scratch DMGs (`rw.106.…dmg` = 160 MB, `rw.66149.…dmg` = 198 MB) that the bundler failed to clean up.
- `release/` adds another 5.1 GB.
- Disk pressure on the SSD can itself slow builds; cleaning `target/` periodically is healthy maintenance.

### Linker

- `which lld`, `which ld64.lld`, `which mold` all return nothing.
- Homebrew `llvm 21.1.8` is installed as a dependency but `lld` is not on `PATH`. The shipped binary lives at `/opt/homebrew/opt/llvm/bin/` (verify with `brew --prefix llvm`).
- Default macOS toolchain linker is being used: `/Applications/Xcode.app/.../usr/bin/ld`.

### sccache

- `rustc-wrapper = "sccache"` is set in `src-tauri/.cargo/config.toml`.
- Local stats: 52 lifetime compile requests, 48 hits, 100% hit rate — i.e. it's working, but very few crates ever go through it. Cargo's own incremental layer fields most rebuilds first; sccache only helps after `cargo clean`, branch switch, or hash invalidation.

## Quick wins (1–2 hours of work, big impact)

### 1. Drop `--bins`: build only the Tauri app

Replace the CLAUDE.md "build before MCP verify" step:

```bash
# old
npx tauri build --debug --features webdriver

# new (inner loop)
npm run build && \
  cargo build --manifest-path src-tauri/Cargo.toml \
    --bin arimalo-covid \
    --features webdriver,tauri/custom-protocol
```

This is roughly equivalent to what `tauri build` does internally, but only links the one binary you'll launch via MCP. `tauri/custom-protocol` is the feature that makes the binary embed and serve `dist/` (without it you get the blank screen that CLAUDE.md warns about).

The binary lands at `src-tauri/target/debug/arimalo-covid` and is directly launchable (MCP/tauri-wd does not need a `.app` wrapper).

If a fully-bundled `.app` is occasionally needed (e.g. for codesign sanity), keep `tauri build --debug --no-bundle --features webdriver` as a sometimes-command — that builds the `.app` but skips DMG creation.

### 2. (Updated) Linker swap — smaller win than first estimated

When I first wrote this doc I assumed `mold` or `ld64.lld` would be a quick install on macOS. Turns out **neither is true today**:

- `brew install mold` succeeds, but mold v2.41.0 prints `Support for Mach-O targets has been removed.` — the Mach-O port was forked off into a commercial product (Sold) and dropped from open-source mold.
- Homebrew's `llvm` package no longer ships `lld`/`ld64.lld` in its `bin/` (only `lldb`). To get lld on macOS you'd `brew install lld` separately (or build it from source).
- Apple's stock `ld` is now `ld-1267` (the new "ld-prime" linker shipped with Xcode 15+) — already 3-5× faster than the old `ld_classic`. So the linker speedup ceiling on modern macOS is much smaller than the Linux story suggests.

Empirical observation: the 8.27 s warm-cache rebuild above was dominated by rustc codegen, not linking — the binary was already linked, and `cargo` only re-emitted the `arimalo-covid` crate's CGUs. Linker swaps wouldn't meaningfully improve this case.

**Recommendation: skip this fix for now.** Revisit only if `cargo --timings` shows linking is a meaningful slice. If you do try it:

```bash
brew install lld
```

```toml
# src-tauri/.cargo/config.toml
[target.aarch64-apple-darwin]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=/opt/homebrew/opt/lld/bin/ld64.lld"]
```

Expect maybe 1-3 s savings on a full-link (the 11 CLI bins, if you ever rebuild them all). Not worth the config maintenance for the 8-second inner-loop case.

### 3. Don't run concurrent builds against the same target dir

If you have two agents / terminals iterating at once they block on cargo's `target/.cargo-lock`. Pick one of:

- Run only one build at a time (simplest).
- Give the second agent its own target dir: `CARGO_TARGET_DIR=$HOME/.cache/arimalo-target-2 cargo build …`.
- Worktrees with separate target dirs (the `Agent({ isolation: "worktree" })` flow already does this).

### 4. Reclaim disk + clean bundler scratch

```bash
# nuke the bundler scratch the DMG step leaves behind
rm -rf src-tauri/target/debug/bundle/macos/rw.*.dmg

# periodically (e.g. weekly):
cargo clean --manifest-path src-tauri/Cargo.toml --release   # 5 GB back
# or full nuke if disk is tight:
cargo clean --manifest-path src-tauri/Cargo.toml             # 80 GB back; next build is slow
```

`cargo install cargo-sweep` lets you remove only artefacts older than N days — good middle ground (`cargo sweep --time 30`).

### 5. Use `cargo --timings` to verify

Once changes are in place, get a real timing breakdown:

```bash
cargo build --manifest-path src-tauri/Cargo.toml \
  --bin arimalo-covid --features webdriver,tauri/custom-protocol \
  --timings
# opens target/cargo-timings/cargo-timing.html in browser
```

This shows per-crate compile time and the critical path. It's the right tool for arguing about where the next 60 s of savings should come from.

## Medium-effort improvements (half a day)

### A. Move the CLI binaries out of the Tauri crate

The 11 `arimalo-*` CLIs live alongside the Tauri app as `[[bin]]` entries in the same crate. Effects:

- Any Tauri-driven `cargo build` walks the whole bin set unless tightly scoped.
- Touching `lib.rs` invalidates incremental state for all 13 binaries.
- The Tauri `dependencies` table (with `tauri`, `tauri-build`, `webview` deps) leaks into the CLI builds, dragging in dependencies that pure-Rust CLIs don't need.

Refactor: make `src-tauri/` a workspace, move the CLIs to `crates/arimalo-cli/` (or one crate per bin), keep shared logic in a `crates/arimalo-core/` lib. Then the Tauri build only links the app and one shared lib. Build cache also stays valid more often.

### B. Add a `[profile.dev-fast]` for inner-loop iteration

```toml
# src-tauri/Cargo.toml
[profile.dev]
debug = "line-tables-only"     # already set

[profile.dev.package."*"]
debug = false                  # no debuginfo for deps — meaningful link-time win

[profile.dev-fast]
inherits = "dev"
debug = 0
incremental = true
opt-level = 0
codegen-units = 256
```

Use with `cargo build --profile dev-fast …`. Tauri's CLI doesn't easily accept a custom profile, which is another reason to step around it for inner loop.

### C. Stop generating split-debuginfo files you never use

`tauri` currently compiles with `-C debuginfo=line-tables-only -C split-debuginfo=unpacked` (visible in the running rustc invocation). The unpacked `.dSYM`-style files inflate `target/debug/deps/` and take real time to emit. For inner-loop iteration:

```toml
[profile.dev]
split-debuginfo = "off"        # macOS default is "unpacked"
```

Keep `debug = "line-tables-only"` — panic backtraces still get line numbers.

### D. Consider `tauri dev --features webdriver` for MCP loops

`tauri dev` runs `cargo run` (so it skips bundling entirely) and points the WebView at the Vite dev server (so frontend edits hot-reload). The MCP `tauri-automation` server connects to `tauri-wd` on port 4444 against any running Tauri process — it doesn't care whether the app was launched from a built `.app` or from `tauri dev`.

The only reason to keep `tauri build --debug` in the MCP loop is if you need to verify production-protocol asset loading specifically. For everything else (UI logic, event handlers, layout) `tauri dev` is 5–10× faster.

CLAUDE.md currently says `tauri build --debug` is mandatory because "cargo build alone does not embed the frontend". That's only true if you forget `--features tauri/custom-protocol`. Update the doc.

## Longer term (1–3 days)

### E. Split `main.rs` (3,462 lines)

It already imports 22 lib modules. Split the Tauri command handlers into their own module file(s) so a typical change touches a few hundred lines, not 3,462. Smaller CGUs = faster incremental compile.

### F. Audit heavy deps

- `rhai` (50 MB rlib) — scripting engine used in `csv_transform.rs`. Powerful but heavy. If the transform DSL is small enough, a hand-rolled mini-evaluator could replace it.
- `automerge` (64 MB rlib, used for CRDT sync) — irreplaceable for the sync feature but only used in `sync.rs` / `automerge_store.rs`. Gate behind a `sync` feature flag so plain `cargo build` (CLIs, tests) can skip it.
- `tera` — only used for `report_templates.rs`. Could potentially move into a sub-crate so CLIs don't pull it in.

### G. Move `target/` to a faster filesystem

On Apple Silicon the internal SSD is fast, but APFS snapshotting + Spotlight can throttle huge directories. Options:

- Add `src-tauri/target/` to Spotlight's privacy list.
- Use a RAM disk for `incremental/` (volatile but rebuilds from scratch if lost):
  ```bash
  diskutil erasevolume HFS+ "RustIncremental" $(hdiutil attach -nomount ram://4194304)
  export CARGO_TARGET_DIR=/Volumes/RustIncremental/arimalo-target
  ```

### H. CI: cache more aggressively

`Swatinem/rust-cache@v2` is already used in `.github/workflows/build.yml`. Two tweaks:

- Set `shared-key: <something-stable>` so PR branches reuse cache from `main`.
- Enable `save-if: ${{ github.ref == 'refs/heads/main' }}` to only write the cache from main and avoid bloat.

## Things I checked but ruled out

- **Release profile is fine.** `lto = true`, `codegen-units = 1`, `opt-level = "s"` are slow, but they only apply to `--release` builds. They don't affect the inner-loop `--debug` path.
- **sccache is configured correctly.** It just doesn't have much to do — cargo's incremental cache fields most rebuilds before sccache sees them. Removing sccache wouldn't help; keeping it costs nothing.
- **Frontend build (Vite) is fast.** 54 TS files, build in single-digit seconds. Not on the critical path.
- **`tauri-build` build script** does some work each build but it's small (<2 s typically).

## How I'd recommend rolling this out

In order, validating each step with a `cargo --timings` run:

1. **Update CLAUDE.md's "verify UI" step** to use `cargo build --bin arimalo-covid --features "webdriver tauri/custom-protocol"` (with a `npm run build` precondition). Document `tauri build --debug --no-bundle --features webdriver` as the rare "verify the .app actually packages" command. *This single change is what produced the 16 min → 8 s result above.*
2. **Clean up the orphaned bundler scratch** in `target/debug/bundle/macos/` (~360 MB of `rw.*.dmg` files).
3. **Add `[profile.dev.package."*"] debug = false` and `split-debuginfo = "off"`** to `src-tauri/Cargo.toml`. Cheap wins on link size + emitter time.
4. **Run `cargo --timings`** once to see the new critical path. Decide whether the workspace split (item A) is worth doing.
5. *(Optional)* Linker swap — only if `--timings` says linking is still the bottleneck. See section 2 for the macOS-specific caveats.

After (1) alone: confirmed **16 min → 8 s** on warm cache; **~2 min** when the cache is hot for `tauri build --features webdriver` but the local crate needs a rebuild; **5–15 min** when going from a cold-for-webdriver state (e.g. straight after `tauri dev`). The reverse trip (MCP-verify build → `tauri dev`) measured **47 s** end-to-end.
After (A): expect cold rebuilds after touching `lib.rs` to drop further because CLI binaries are out of the graph.

## Quick health-check commands

```bash
# how big is target/?
du -sh src-tauri/target/{debug,release,llvm-cov-target}/

# what's cargo about to do? (dry run + verbose)
cargo build --manifest-path src-tauri/Cargo.toml \
  --bin arimalo-covid --features webdriver,tauri/custom-protocol \
  --timings -v

# any concurrent builds?
ps -ef | grep -E "(cargo build|rustc)" | grep -v grep

# sccache effectiveness
sccache --show-stats
```
