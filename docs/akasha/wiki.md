# The wikis and docs

Mirrored by `tools/summon-wiki.py` (MediaWiki API) into `upstream/wiki/<site>/`
(gitignored, like the rest of `upstream/`). Re-runs are incremental.

| Source | Where | Status | Licence |
|---|---|---|---|
| Frontier wiki | `frontierstation.wiki.gg` → `upstream/wiki/frontier` | ✅ 613 pages (51 articles), 232 images, 9.2 MB, pulled 2026-10-11 | CC BY-SA 4.0 |
| SS14 wiki | `wiki.spacestation14.com` → `upstream/wiki/ss14` | ⛔ origin down (Cloudflare 530) on 2026-10-11; `web.archive.org` resets connections from the container | CC BY-SA 3.0 |
| SS14 dev docs ("The Robust Book") | `github.com/space-wizards/docs` → `upstream/ss14-docs` | ✅ 326 pages, `c9f9aa8` | see repo |
| In-game guidebooks | `upstream/{ss14,frontier}/Resources/ServerInfo/` | ✅ 204 (SS14) / 350 (Frontier) XML docs | CC BY-SA 3.0 |

Layout of a wiki mirror:

```
siteinfo.json, index.json, images.json
wikitext/<ns>-<name>/<Title>.wiki   source of every page in every namespace
html/<Title>.html                   rendered main-namespace articles
images/<File>                       every uploaded file
```

Quick text view of an article:

```bash
python3 -I tools/wiki-text.py upstream/wiki/frontier/html/Piloting.html
```

## What's useful where

- **Frontier wiki**: the player-facing loop (salvage, shipyard, expeditions,
  sector layout, piloting). Digest in [systems/frontier-gameplay.md](systems/frontier-gameplay.md).
- **Guidebooks**: authoritative mechanics written by the devs, shipped with the
  game; `_NF/Guidebook` has Frontier's (sector topology, shipyard, expeditions...).
- **Dev docs**: how the engine/code are designed: ECS, transforms/grids, physics,
  node networks (power/pipes), device network, NPCs, body, chemistry, construction,
  destructible, entity tables. Read the relevant chapter before porting a system.

## To do

- Retry the SS14 wiki (`tools/summon-wiki.py ss14`) when the origin is back,
  or via the Wayback Machine if the network allows it.
