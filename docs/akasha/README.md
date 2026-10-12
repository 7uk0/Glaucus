# The Akasha

The Akashic record: everything this project has learned, written down so no
session starts from zero. **Read this first. Append to it as you learn.**

Sessions that work on Caput Mortuum run in throwaway containers. Anything
not committed here is forgotten. The rule is simple: when you learn how an
upstream system works, why it exists, a trap in the build, or a decision the
user made, write it down in the right page below, in the same commit as the
code.

## Pages

| Page | What's in it |
|---|---|
| [next.md](next.md) | **Start here:** current state, what to do next, open threads, handy commands |
| [user.md](user.md) | Who the user is, how they want to work, decisions they made |
| [journal.md](journal.md) | Chronological log of sessions: what was done, what's next |
| [environment.md](environment.md) | Build/run/test gotchas, disk, headless rendering, network |
| [upstream.md](upstream.md) | Map of the upstream codebases: where things live, how to find them |
| [systems/](systems/) | One page per game system: what it's for, how upstream does it, how we ported it. Atmospherics, power, worldgen, docking, rendering, mining (research), Frontier gameplay |
| [wiki.md](wiki.md) | The SS14 and Frontier wikis: where they are, how they were pulled, what's in them |

## Writing rules

- **Facts with sources.** Cite the upstream file (`Content.Server/.../FooSystem.cs`)
  or wiki page. "Upstream does X" must come from reading upstream, not memory.
- **Numbers verbatim.** Defaults, constants and CVars exactly as upstream has them.
- **Why, not just what.** Each system page opens with what the system is *for*
  in play, because that decides what matters when porting.
- **Short entries, dated.** Journal entries are dated `YYYY-MM-DD`.
- Keep the esoteric naming theme for new crates and tools (see `CLAUDE.md`).
