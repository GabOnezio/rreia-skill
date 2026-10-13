# Installing the skill

A skill is a folder with a `SKILL.md` (name and description in a YAML header, then instructions) plus optional `scripts/`, `references/` and `assets/`. Nothing to compile. The agent reads the description to decide when to use it, and loads the rest on demand.

## Claude Code and Claude CLI

Copy the whole `rreia/` folder to one of:

- `~/.claude/skills/rreia/` (all your projects)
- `<project>/.claude/skills/rreia/` (one project, shareable through git)

Type `/rreia` to run it explicitly, or just describe the work and the agent will pick it up from the description.

## OpenAI Codex

Codex reads the same `SKILL.md` format. Documented locations (from a third-party description; the official page could not be read when this skill was written, so confirm in Codex's own documentation):

- `<project>/.agents/skills/rreia/`
- `~/.agents/skills/rreia/`

The invocation syntax differs from Claude Code's `/`; use the skills listing in Codex to find it. If your Codex version does not load skills, copy the "Flow" section of `SKILL.md` into the project's `AGENTS.md`; every agent that reads `AGENTS.md` will then follow the interview and compatibility steps, without the scripts.

## Test that it loaded

Ask: "I want an offline assistant on a Raspberry Pi 5. Use RREIA." The agent should ask whether the program will run on the current device, then ask for or probe the hardware, then give a verdict with L1 to L3 numbers before writing code.
