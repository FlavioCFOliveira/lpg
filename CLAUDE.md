# CLAUDE.md

Operating instructions for Claude Code and its agents and subagents in this repository.

## Sources of truth

- `README.md` defines the project at a high level: what it is, its scope and its requirements. Read it before any design or implementation work, and do not restate or redefine project characteristics here.
- This file covers only how Claude Code and the agent ecosystem operate in this repository.

## Guiding principle

**Maximise the return on effort: deliver the most with the least work.** Apply it by default to all work in this repository, without being asked. It is a way of working, not a task: never record it in `rmp`.

## Honest empiricism (mandatory)

> **Every decision, claim and answer in this repository must rest on measurable data or on official sources. Speculation is forbidden.**

- **Act on evidence only.** Base all work on measured data (tests, benchmarks, profiles, reproducible experiments) or on information from official sources (specifications, standards, official documentation, source code).
- **Never guess.** Do not fill gaps with assumptions. When evidence is missing, obtain it by measuring or by consulting an official source; if neither is possible, state explicitly that the point is unverified.
- **Never tell the user what they want to hear.** Report what the evidence shows, including results that are negative, inconvenient or contrary to the user's expectations.
- **Never present the most plausible answer as a fact.** Plausibility is not evidence.
- **Cite the evidence.** Every non-trivial claim must identify its basis: the measurement and how it was obtained, or the source and where in it.
- **Report outcomes faithfully.** Failed tests, regressions, uncertainties and skipped steps must be stated plainly, never hidden or softened.

## Laboratory development (primary principle for code)

> **Treat all code in this project as permanently under laboratory conditions: observe it, measure it, evaluate it, then improve it.**

This is the primary principle of code development in this repository. Apply it as a continuous cycle:

1. **Observe.** Profile the code empirically with the appropriate tools: CPU, memory (RAM and allocations), storage (I/O, space, durability behaviour) and any other relevant system resource.
2. **Evaluate.** Judge the code from the profiling results, not from intuition or expectation.
3. **Improve.** Change the code based on solid, honest and reliable knowledge (see [Honest empiricism](#honest-empiricism-mandatory) and [Open-source references](#open-source-references)).
4. **Verify.** Profile again and confirm the improvement with measured results before accepting the change.

Profiling tooling:

- For each need, use the most effective tool available, from flame graphs to memory and heap dumps and any other suitable instrument.
- If the best tool for a need is not installed or not available, ask the user to make it available. Do not silently fall back to a weaker tool.

## Simplicity (code strategy)

> **Avoid over-engineering. Always choose the simplest solution that meets the project's strict needs.**

- Implement only what a current requirement needs. Do not add abstractions, generality, configuration or extension points for hypothetical future needs.
- Prefer the simpler of two solutions that both meet the requirement. Accept added complexity only when measured evidence shows the simpler solution does not meet a stated requirement (see [Laboratory development](#laboratory-development-primary-principle-for-code)).
- Apply this criterion to the functional and technical specifications and to every design decision: specify only what the project needs, and choose the simplest design that satisfies the specified requirements.

## Synergy and convergence

- **Group related tasks.** Whenever tasks (in `rmp` or requested ad hoc by the user) have verifiable functional or technical proximity, or complementary or convergent goals, combine them into a single development effort.
- **Group work of the same type, across and within tasks.** Write all the code at once, then test all the changes at once, instead of writing and testing small pieces in isolation. Write all the documentation at once; if its scope is too large, split it into a few coherent blocks and handle each block at once. Apply this principle to every kind of work.
- **Minimise tasks and iterations.** Reach every goal with the fewest possible tasks and iterations.
- **Never trade quality for speed.** Aggregate work only when the result is faster and of equal or better quality than task-by-task delivery.
- **Use available resources fully.** Make the most of the internal resources available, so that delivery is faster and costs the user less.

## Open-source references

> **Copying is forbidden.** Never copy code, text or any other content from another project. Consult other projects only to study them and draw inspiration from their reference approaches, so that this project's own implementation is of the highest quality.

- **Sole exception:** the `opengql/grammar` project (Apache-2.0) may be used as a grammar and test artefact for GQL conformance verification. Preserve its licence and attribution unchanged.

- Base solutions on proven open-source projects whose licence permits reading their source code for inspiration. Use their strategies, architectures and approaches as starting points instead of reinventing them from scratch.
- Consult these references:
  1. **Rust crates**: every crate that already implements any functionality this project needs, with priority to the most widely used and recognised crates.
  2. **Open-source databases**: the approaches of the most recognised databases for the relevant task or functionality, such as Memgraph, Neo4j, PostgreSQL, InnoDB, MariaDB and MySQL.
  3. **Other open-source projects**: any project that reveals a relevant strategy, architecture or approach.
- Follow this decision order for every code and architecture decision:
  1. **Learn first.** The first approach must never reinvent the wheel. Make an informed decision by studying the references above and identifying **only the best** approaches of each project, then implement them originally in this project.
  2. **Improve second.** Only when that process reveals a measurable improvement opportunity (see [Honest empiricism](#honest-empiricism-mandatory)), proceed to improve on the chosen approach.
- Treat source code as the most reliable source of truth. Clone each reference repository locally under `/tmp/` (for example, `/tmp/<project>`) and read the code directly, for study only.

## Subagents

- Delegate the execution of **all** work to the subagent that is best specialised in the goals of that work. Always choose the most suitable subagent.
- Run **exactly one** subagent at a time, in parallel with the main Claude Code conversation. Never run more than one subagent concurrently.
- Use as many subagents as the goal requires, strictly **in series**.
- If the user authorises more than one concurrent subagent, the authorisation is an exception that applies only to the current task and is revoked when that task ends.

## Language

- Write and interpret instructions so that they are always:
  - **Explicit**: the intent is clear.
  - **Objective**: what must be executed is always known.
  - **Closed**: the scope of the work is bounded.
  - **Concise**: few words are used.
- **All documentation**, from the main `README.md` to the specification and code documentation, must be written in flawless, professional English, free of spelling, grammar and syntax errors, and must follow the four rules above.
- **All documentation** produced in this project must be absolutely:
  - **Simple**: no unnecessary complexity, jargon or ornament.
  - **Clear**: unambiguous and easy to understand on first reading.
  - **Honest**: states only what is true and verified, including limitations (see [Honest empiricism](#honest-empiricism-mandatory)).
  - **Objective**: factual, free of opinion, promotion or exaggeration.

## Scope discipline

- Stay strictly focused on the goal of the current work.
- Never start a task that was not **explicitly** requested.
- When a need outside the scope of the current task is identified, ask the user how to proceed; never start it proactively.

## Completeness

- Never execute work partially. Every task that is started must be completed in full.
- **Never leave a task half done or partially done.**

## Skills

- **`gitflow`**: execute every git write command through this skill, following the gitflow branching model for this repository.
- **`roadmap-manager`**: use this skill to coordinate and manage tasks, sprints and comments, and for every `rmp` CLI operation except `rmp graph *`.
- **`knowledge-authority`**: manage the project knowledge graph exclusively through this skill, including every `rmp graph *` operation.

## Git

- Default remote: `xumiga` (`git@wg32:/xumiga/lpg.git`); `main` tracks `xumiga/main`.
- Mirror remote: `Github` (`git@github.com:FlavioCFOliveira/lpg.git`); use it only when explicitly asked.
- Execute every git write operation through the `gitflow` skill (see [Skills](#skills)).
