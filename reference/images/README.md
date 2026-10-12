# Reference images

Images of how the real game looks, used to compare against our renderer.
Downloaded 2026-10-12. None of it is ours. Every file was checked to be a
real PNG, JPEG or WebP before it was kept. Each folder has an `index.json`
that gives every file's source URL, size, dimensions, sha256 and (where the
source says) its render date and the upstream map file it shows.

Total: 184 images, about 101 MB.

| Folder | Files | Size | What it is |
|---|---|---|---|
| `frontier-ships/` | 112 PNG | 20 MB | Top-down full-ship renders of Frontier shuttles, from Content.MapRenderer |
| `ss14-maps/` | 20 PNG + 27 WebP | 70 MB | 20 Frontier POI/outpost renders, plus 20 SS14 station renders (27 grids) |
| `screenshots/` | 10 JPEG + 6 JPEG + 9 PNG | 13 MB | Gameplay screenshots of SS14 (Steam store page and spacestation14.com) |

## Naming

- **`frontier-ships/<basename>.png`**: `<basename>` is the shuttle's map file
  name, so `ambition.png` is `Resources/Maps/_NF/Shuttles/ambition.yml`. All
  112 files match a map in `upstream/frontier` at the pinned commit (the
  `map` field in `index.json` gives the full path, including subfolders such
  as `Nfsd/`, `Scrap/` or `BlackMarket/`).
- **`ss14-maps/<basename>.png`**: Frontier POIs and the outpost, named after
  their map file (`frontier.png` is `Maps/_NF/Outpost/frontier.yml`,
  `cove.png` is `Maps/_NF/POI/cove.yml`). The shipyard site calls two of
  them by other names: `beacon.png` is the site's "Omnichurch", and
  `arena.png` is its "ThePit".
- **`ss14-maps/<mapId>.webp`**: SS14 stations, named after their map ID
  (`box.webp` is `Resources/Maps/box.yml`). This file is the main (largest)
  grid. Each other grid on the map is saved as `<mapId>.grid<gridId>.webp`;
  for example, `oasis.grid1588.webp` is Oasis's planet grid. In `index.json`,
  `gridOffset` gives each grid's world offset and `inPinnedSs14` /
  `inPinnedFrontier` say whether that map still exists in our pinned
  upstream.

## Sources and licences

| Source | Used for | Licence / attribution as stated |
|---|---|---|
| Frontier Shipyard render site, https://shipyard.frontierstation14.com (built by Forged, https://forged.by/projects/shipyard-mapviewer; linked from the Frontier wiki's Shipyard page). Index pages `/render/shuttles` and `/render/poi`; images at `/storage/renders/shuttles/<file>` and `/storage/renders/poi/<file>` | `frontier-ships/` (112), `ss14-maps/` Frontier POIs (20) | The site states no licence. The images are Content.MapRenderer output of Frontier map files (AGPL-3.0 for the `_NF` maps; see Frontier `LEGAL.md`) drawn with SS14/Frontier sprites (mostly CC-BY-SA 3.0, per each RSI's `meta.json`). Attribution: Frontier Station contributors / new-frontiers-14. Server `Last-Modified`: ship renders July 2026 (`lanternmk2` April 2026); POI renders 2025-11-28. |
| SS14.MapServer instance at https://mapserver.tanukij.dev (it backs the map viewer https://maps14.tanukij.dev, the reference deployment of space-wizards/ss14.mapviewer). List from `/api/Map/list/master`, metadata from `/api/Map/<id>/master`, images from `/api/Image/grid/<mapGuid>/<gridId>` | `ss14-maps/*.webp` (20 stations, 27 grids) | The server states no licence (`attribution: null`). The images are Content.MapRenderer output of SS14 maps (MIT code, CC-BY-SA 3.0 sprites unless an RSI says otherwise). The server does not name the repository it renders. Its 20 maps match the upstream space-wizards/space-station-14 station rotation of 2023–2024, so they are almost certainly upstream `master`. **Render dates: 2023-06-07 to 2024-10-09**, much older than our pinned commits. |
| Steam store page, Space Station 14 (app 1255460), https://store.steampowered.com/app/1255460/Space_Station_14/. URLs from `https://store.steampowered.com/api/appdetails?appids=1255460`, files from `shared.akamai.steamstatic.com` | `screenshots/steam-00.jpg`–`steam-09.jpg` (1920×1080) | No licence stated. Developer and publisher: Space Wizards Federation. These are promotional screenshots, kept here for comparison only. |
| spacestation14.com home page gallery, `https://spacestation14.com/images/home/gallery-*.jpg` | `screenshots/ss14site-gallery-{bar,clown,engineering,medbay,nukies,singulo}.jpg` | No licence stated on the site. © Space Wizards Federation / SS14 contributors. |
| spacestation14.com progress reports 37–39 (2022-10-31, 2022-11-30, 2023-03-06), `https://spacestation14.com/post/<slug>/`, images `https://spacestation14.com/images/post/pr_<n>/<file>` | `screenshots/ss14site-pr-<n>-<file>.png` (9 picked from 36: in-game lighting, HUD and chat, the lobby, character setup, a shuttle) | As above. They show the game as it was in 2022–2023. |

The pinned upstream commits (from `upstream.lock`) are frontier `a4a6f4b2`
and ss14 `ac7a32ec`. Some details will differ from these renders, most of all
in the SS14 stations: Aspid, Atlas, Barratry, Cluster, Core, Gemini, Kettle,
Meta, Moose, Omega, Origin and Train are no longer in either pinned checkout.
Bagel, Box, Marathon, Packed and Saltern are in both. Fland, Oasis and Reach
are in Frontier only.

## What is missing

- **Ships with no render here**: `fishbowl`, `warden` (admin) and `dendron`.
  The shipyard lists all three, but their image URLs return 404. The site
  does not list `point` or the scrap variants `scrap-barge`, `scrap-comet`,
  `scrap-courserx`, `scrap-crescent`, `scrap-hauler`, `scrap-investigator`,
  `scrap-sparrow` and `scrap-sprinter` at all.
- **POIs with no render here**: `minicentcomm` (listed, but the image returns
  404) and `cargodepotalt` (not listed).
- **SS14 stations in our pinned checkout with no render**: amber, elkridge,
  exo, plasma, relic, snowball, sushi and tram2. No reachable source had them.
- **No Frontier-specific gameplay screenshots.** No official source we could
  reach has any. The Frontier wiki's screenshots are already in
  `../wiki/frontier/images/`.
- User renders on the shipyard site (`/user-renders`) were skipped. They are
  player-made and not tied to a map file.

## Sources that failed

| Source | What happened |
|---|---|
| Official SS14 map server and viewer: `mapserver.spacestation14.com`, `map.spacestation14.com`, `maps.spacestation14.com` | DNS does not resolve (`ENOTFOUND`; the proxy answers 502 to CONNECT). We used the tanukij.dev mirror instead. |
| SS14 wiki: `wiki.spacestation14.com`, and `wiki.spacestation14.io`, which redirects there | Cloudflare 530, the same as on 2026-10-11. |
| Wayback Machine (`web.archive.org`), tried for archived mapserver images | The egress proxy closed the connection (reset after CONNECT). |
| GitHub API and github.com web pages (to search the `new-frontiers-14` and `space-wizards` orgs for render repos) | Blocked for repositories not attached to this session (HTTP 403). Plain `git clone` of public repos and `raw.githubusercontent.com` did work. We found no repository that publishes render PNGs. |
| GitHub `space-wizards/website` (source of spacestation14.com) | `git ls-remote` asks for credentials: the repository is private or does not exist. We used the live site instead. |
| frontierstation14.com (Squarespace) | Reachable, but it has only logos and backgrounds, no gameplay screenshots. Nothing was kept. |
