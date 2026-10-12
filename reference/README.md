# Reference material

Copied here so it survives with the code. None of it is ours.

| Folder | Source | Licence |
|---|---|---|
| `wiki/frontier/` | Frontier Station wiki, https://frontierstation.wiki.gg, mirrored 2026-10-11 by `tools/summon-wiki.py frontier`: 613 pages (source + rendered articles), 232 images | CC BY-SA 4.0 (see `wiki/frontier/siteinfo.json`); authors per page history on the wiki |
| `ss14-docs/` | Space Wizards developer docs, https://github.com/space-wizards/docs @ `c9f9aa8ed6074a6751609b6e1b4f4211c7a293ac` | See that repository |

The SS14 player wiki (wiki.spacestation14.com) could not be mirrored: its
server returned Cloudflare 530 throughout 2026-10-11. Retry with
`tools/summon-wiki.py ss14`.

The game itself (Frontier, SS14, RobustToolbox, ~1.6 GB) is not copied: it is
fetched at the pinned commits in `upstream.lock` by `tools/summon-upstream.sh`.

## images/

Reference images of the real game for renderer comparisons: 112 Frontier
ship renders, 20 Frontier POI/outpost renders, 20 SS14 station renders and
25 gameplay screenshots. Sources, licences and per-file provenance are in
[images/README.md](images/README.md) and each folder's `index.json`.
