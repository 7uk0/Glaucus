# The user

- GitHub: `7uk0`. Address them as **User**. Pronouns not stated: use they/them.
- Home lab builder (Debian 13 machines "Wolf-Box", "Calavera-Orbital", "Fox-Box";
  a local AI called Theta on OpenWebUI). Wolf-Box: Ryzen 7 3700X, RTX 3060 Ti,
  3440x1440 ultrawide, Xfce4/Chicago95.
- Loves esoteric / alchemical / psychedelic naming. Crate and tool names follow
  that theme (vellum, grimoire, astrolabe, athanor, homunculus, mercury, alembic,
  akasha...).
- Communication: when troubleshooting, be direct and functional. When
  brainstorming, warmth and real connections between esoterica and tech are welcome.

## Requests and decisions (verbatim where it matters)

- 2026-10-11: "We are doing a FULL rust Rewrite all of ss14 pull it and frontier
  station's branch and for single player with npcs get to work"
- 2026-10-11: chose **a new repo** for the rewrite → `7uk0/Caput-Mortuum`
  (they must create it; the GitHub integration cannot create repos: 403).
- 2026-10-11: "Save memory as you go. Make sure to learn how all aspects of the
  game and code work and what for, look things up if needed, dont stop till done.
  Save memory and code. Pull images and full wiki of ss14 and frontier station."
  → this Akasha; code delivered as git bundles until the GitHub repo exists;
  wikis mirrored by `tools/summon-wiki.py`.

## Standing instructions

- Develop on branch `ccr-9978bc94-j2gdmx`; push with `git push -u origin <branch>`;
  open a **draft** PR after pushing; never put model names in commits/PRs.
- Commit footer:
  `Co-Authored-By: Claude ... <noreply@anthropic.com>` + `Claude-Session: <link>`
  (use the exact lines the session's system reminder gives).
- License is AGPL-3.0 (inherited from Frontier); keep MIT notices for SS14/RT.
