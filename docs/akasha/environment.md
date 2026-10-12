# Environment and build gotchas

## Toolchain

- Rust **1.97.1+** required (`wgsl-types` in Bevy 0.20's tree). `rustup update`
  brought the container to 1.99. Edition 2024, resolver 3.
- Bevy **0.20** (`default-features = false`; the client enables `2d`, `ui`, `audio`).
  API notes for 0.20: events are `Message` / `MessageReader` / `MessageWriter`
  (`world.write_message`); observers use `EntityEvent` + `On<T>`;
  `bevy::ui_widgets` buttons fire `Activate` and take `.observe(...)`.
- YAML is parsed with `yaml_serde` 0.10 but through our own `NodeVisitor`
  (`grimoire::node`): its `Value` type rejects numbers like `340282300000000000000000000000000000000`
  (a gas `boilingPoint`), so never deserialize straight into `yaml_serde::Value`.

## System packages (Debian/Ubuntu)

- Build: `pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`
- Headless run/screenshots: `xvfb mesa-vulkan-drivers libxkbcommon-x11-0` (lavapipe)

## Headless screenshots

```bash
xvfb-run -a -s "-screen 0 1600x900x24" env WGPU_BACKEND=vulkan \
  target/debug/caput-mortuum --screenshot out.png --frames 150
```

ALSA errors in the log are harmless (no sound card). For big maps, give Xvfb
a big screen (`2600x2600x24`): `--render-map` sizes the window to the map.
Don't rebuild `caput` while a background batch render is running, or later
launches will use the new binary. Parallax must sit inside
the camera clip range (z = -990, not -2000).

## Disk (cloud container)

- The writable allowance is ~40 GB; `df` "Avail" is what's left of it.
- **One cargo target dir only.** A second `CARGO_TARGET_DIR` doubles usage
  (13 GB + 10 GB filled the disk on 2026-10-11). Use the repo's `target/`.
- `upstream/` holds symlinks into the scratchpad (Frontier ~1 GB, RT, SS14).
- Old client binaries pile up in `target/debug/deps/caput_mortuum-*` (~1.7 GB
  each, one per rebuild). On 2026-10-12 `deps` reached 20 GB and the disk had
  2.4 GB left. Delete all but the newest ones, plus `target/debug/incremental`.
  Also delete test executables in `deps` older than about 2 hours, then rebuild.

## Shell / harness

- `pkill` of a pattern matching the shell kills the tool's shell. Don't.
- Foreground `sleep` polling is blocked; use background tasks (`run_in_background`).
- `cd` in one Bash call doesn't persist reliably; use absolute paths.
- Moving the workspace: it was first created under `/root` by mistake; the
  repo lives at `/home/user/Caput-Mortuum`.

## Network

- Outbound HTTPS goes through an agent proxy; GitHub public clones work.
- `wiki.spacestation14.com` returned Cloudflare **530** (origin down) on
  2026-10-11; `frontierstation.wiki.gg` API works. See [wiki.md](wiki.md).

## GitHub

- Session scope: `7uk0/{Theta,Glaucus,Neural-Link,Phantasmagoric-Manuscritum,Hallow-Moon,Calavera-Orbital}`.
- `7uk0/Caput-Mortuum` does not exist yet; `create_repository` → 403
  ("Resource not accessible by integration"). The user must create it and
  grant the Claude GitHub App access; then `add_repo` (access `push`).
- Until then the code is delivered to the user as `caput-mortuum.bundle`
  (`git clone caput-mortuum.bundle` restores full history).

## Tests

- `cargo test --workspace` (exclude `caput` to skip the client build).
  Upstream-backed tests skip when `upstream/` is missing.
- A full run takes about a minute after compilation; the 145-maps test is the slowest.
