# Legal

## Code

Caput Mortuum is a Rust reimplementation of Space Station 14, RobustToolbox and
Frontier Station 14. It ports behaviour, formulas and data formats from those
projects, so it is a derivative work and follows their licenses:

- **Frontier Station 14** is licensed as a whole under the **AGPL-3.0**
  (see its `LEGAL.md`). Caput Mortuum is therefore licensed under the
  **GNU Affero General Public License v3.0** — see [`LICENSE`](LICENSE).
- **Space Station 14** content code and **RobustToolbox** are MIT-licensed.
  Their copyright notices are preserved in
  [`LICENSE-MIT-SS14.txt`](LICENSE-MIT-SS14.txt) and
  [`LICENSE-MIT-RobustToolbox.txt`](LICENSE-MIT-RobustToolbox.txt) and must not
  be removed from distributions.

## Content

This repository contains no upstream art, audio, maps or prototypes. They are
fetched at build time by `tools/summon-upstream.sh` into `upstream/` (ignored by
git) and read from there. Each asset keeps its own license — mostly
CC-BY-SA 3.0, some CC-BY-NC-SA 3.0 — as recorded in its `meta.json` or the
`attributions.yml` beside it. Anyone distributing a build together with those
assets must follow those licenses.

Files under [`resources/`](resources) are original to Caput Mortuum and are
AGPL-3.0 like the code.

## Screenshots

Images under [`screenshots/`](screenshots) show Space Station 14 / Frontier
Station art rendered by Caput Mortuum. That art is © its respective authors and
licensed CC-BY-SA 3.0 (see the upstream `meta.json` files); the screenshots are
shared under the same license.
