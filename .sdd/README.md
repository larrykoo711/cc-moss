# .sdd: the design archive

This folder is the maintainer's working archive for cc-moss: designs, plans, task specs, review notes and lessons,
written in Chinese while the project was built spec-first. You do not need it to use or contribute to cc-moss;
[CONTRIBUTING.md](../CONTRIBUTING.md) has everything for that.

If you are curious how it works inside, three files are worth reading:

| File | What it holds |
|------|---------------|
| [`001-cli/DESIGN-cli.md`](001-cli/DESIGN-cli.md) | The design of the terminal edition: data model, session state machine, commands, risks |
| [`001-cli/notes/hook-probe.md`](001-cli/notes/hook-probe.md) | What Claude Code's hooks actually do, measured (the facts the state machine rests on) |
| [`001-cli/LESSONS.md`](001-cli/LESSONS.md) | Mistakes made along the way and how the project now avoids them |

`001-cli/` is Stage 1, the terminal edition. Each task's spec lives in `001-cli/tasks/<state>/`, where the folder
is its state (draft, todo, in-progress, done). Code comments that mention a task id (for example `T1.5`) point at
these specs.
