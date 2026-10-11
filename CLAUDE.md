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

## Graph data files in `/tmp/lpgram`

> **When the graph is exercised, its data files live in `/tmp/lpgram/`. Only its data files.**
> **`/tmp` is a RAM-backed `tmpfs` on this machine. It only accelerates file I/O during development. It is never a realistic production scenario and must never be treated as one.**

- **Correctness first (mandatory).** `/tmp/lpgram` must never reduce the effectiveness of any test or scenario; it may only make it faster. When a test or scenario depends on behaviour that `tmpfs` does not reproduce (for example real `fsync`, disk-full or file-system-specific behaviour), or when `tmpfs` would weaken it in any other way, **do not use `/tmp/lpgram` for it**: run it on persistent storage. When in doubt, do not use it.
- **Scope.** Subject to the rule above, every exercise of the graph — experiments, tests, reproductions, error validation, examples (when they exist) and similar runs — creates and uses its graph data files (WAL, snapshots, graph store, indexes and any other file the graph persists) in `/tmp/lpgram/`.
- **Location.** `/tmp/lpgram/`, created with `mkdir -p` when missing; no mount and no root are needed. Create a separate subdirectory per run and delete it when the run ends.
- **Size.** No fixed space limit applies to `/tmp/lpgram/`. Every test and scenario must be no larger than strictly necessary for its own validations: create only the data, files and iterations its assertions need. `/tmp` is shared with the system and enforces user quotas, so measure the peak space a run uses.
- **Clean environment.** Whenever scenarios must be cleared and rebuilt from a clean state, run `rm -rf /tmp/lpgram && mkdir -p /tmp/lpgram`; only do so when no run is using it. Never delete anything else in `/tmp`.
- **Exclusive content.** `/tmp/lpgram/` holds only the graph data files of the run in progress. Never place in it build output (`target/`), caches, logs, executables, reports, evidence, reference clones or any other file.
- **Purpose.** `/tmp/lpgram` only accelerates the file I/O of this crate, so that exercises are faster to create, run and observe. It has no other role.
- **Not a production scenario.** Never consider, design for, document or present `/tmp/lpgram` as a realistic production setup, nor use results obtained in it to describe production behaviour. The crate is designed to run on persistent storage. On `tmpfs`, data does not survive a reboot or power loss and `fsync` does not reach a disk, so results obtained in `/tmp/lpgram` say nothing about real-disk durability or I/O performance. Any claim about those properties must be measured on persistent storage and must state that it was.

## Build output in RAM

- **Location.** Cargo build output goes to `/dev/shm/lpg-target/`, a RAM-backed `tmpfs`, set by a local `.cargo/config.toml` (`[build] target-dir`) that is excluded from git (user authorisation, 2026-10-11). Never put build output in `/tmp/lpgram/`.
- **Scope.** It applies to every cargo workspace in the repository (`.`, `fuzz/`, `tools/gql-oracle/`), and so to every build, test, coverage, fuzz and `loom` run.
- **Transient tool output.** Intermediate files of tools (coverage JSON and `profraw`, `heaptrack` recordings, perf and flame-graph data, logs) go to the session scratchpad or `/tmp`, both `tmpfs`, never to the repository on disk. Only the final evidence is written to `docs/evidence/`.
- **Purpose.** It only speeds up builds. The same limits as `/tmp/lpgram` apply: it is not persistent and says nothing about production behaviour.

## RAM space discipline (mandatory)

RAM-backed space (`/tmp/lpgram/`, `/dev/shm/lpg-target/`) is scarce and shared. Delete every file there as soon as it is no longer needed (user decision, 2026-10-11):

- **Graph data:** delete each run's subdirectory when the run ends, including after a failure.
- **Build output:** delete single-use build directories (for example `--target-dir` variants for `--cfg fuzzing`, `cargo llvm-cov` or `loom` builds) as soon as their gate or measurement finishes. Keep only the build in current use.
- **Close of a sprint:** delete `/dev/shm/lpg-target/` when no further build is planned.
- Measure the space before and after every heavy run, and report what was left and why.

## Simplicity (code strategy)

> **Avoid over-engineering. Always choose the simplest solution that meets the project's strict needs.**

- Implement only what a current requirement needs. Do not add abstractions, generality, configuration or extension points for hypothetical future needs.
- Prefer the simpler of two solutions that both meet the requirement. Accept added complexity only when measured evidence shows the simpler solution does not meet a stated requirement (see [Laboratory development](#laboratory-development-primary-principle-for-code)).
- Apply this criterion to the functional and technical specifications and to every design decision: specify only what the project needs, and choose the simplest design that satisfies the specified requirements.

## Synergy and convergence

- **Group related tasks.** Whenever tasks (in `rmp` or requested ad hoc by the user) have verifiable functional or technical proximity, or complementary or convergent goals, combine them into a single development effort.
- **Group work of the same type, across and within tasks (mandatory).** When several tasks need activities of the same type, do that activity for all of them at once, never task by task:
  - **Code:** write the code of every task in one pass.
  - **Tests:** test every resulting implementation in one run, never test by test.
  - **Documentation:** write it all at once; if its scope is too large, split it into a few coherent blocks and handle each block at once.
  - **Every other operation** in this project (specification, audits, measurements, `rmp` updates, commits): the same rule.

  Apply it only when the delivery keeps its full quality and effectiveness; where grouping would compromise either, split the work and state why.
- **Minimise tasks and iterations.** Reach every goal with the fewest possible tasks and iterations.
- **Never trade quality for speed.** Aggregate work only when the result is faster and of equal or better quality than task-by-task delivery.
- **Use available resources fully.** Make the most of the internal resources available, so that delivery is faster and costs the user less.

## Open-source references

> **Copying open-source code is allowed under the rules below.** Study other projects to learn their best approaches, and copy their code when that serves this crate better than writing it anew.

- **Copying code.** Code from an open-source repository may be copied, in whole or in part, when **both** hold:
  - its licence is compatible with this project's MIT licence (for example MIT, Apache-2.0, BSD, public domain); never copy GPL-only code;
  - it is useful to the evolution of the crate as a whole or of one of its parts.
- **Ports from other languages.** The same rules apply when the source project is written in another language: its code, concept or solution may be reimplemented, translated or ported to Rust in this crate. Such a port is a copy.
- **Use and evolution of a copy.** A copy may be used exactly as copied, or it may evolve with the project. Keeping it identical to the original version is not a requirement.
- **Obligations for every copy:**
  - keep the copyright and licence notices that its licence requires;
  - record its source (project, path, commit) where it is copied and in the task's `rmp` log;
  - subject it to every rule of this file, including [Laboratory development](#laboratory-development-primary-principle-for-code).

- **GQL conformance test artefacts:** the following Apache-2.0 projects may be used as test artefacts for GQL conformance verification. Preserve their licence and attribution unchanged.
  - `opengql/grammar`: the GQL grammar, including a parser generated from it in a test-only crate.
  - `opengql/tck`: the GQL technology compatibility kit.
  - The openCypher TCK: only the scenarios applicable to GQL.
  - `gql-compat`: its mandatory and condition cases only, as an input to lpg's own scenarios, translated and attributed (user decision, 2026-10-08).
- **ISO electronic inserts:** the official ISO/IEC 39075:2024 electronic inserts (grammar, GQLSTATUS conditions, features, implementation-defined and implementation-dependent items) are stored unmodified in `docs/standards/iso-iec-39075/`. They are a supplementary source for the tables the grammar does not contain (GQLSTATUS conditions, feature identifiers, implementation-defined items), not the grammar authority. Never edit them.
- **GQL reference:** the ISO/IEC 39075:2024 text is not available to this project. The trusted reference for GQL syntax is the official community ANTLR4 grammar, `opengql/grammar` (<https://github.com/opengql/grammar>).

- Base solutions on proven open-source projects whose licence permits reading their source code for inspiration. Use their strategies, architectures and approaches as starting points instead of reinventing them from scratch.
- Consult these references:
  1. **Rust crates**: every crate that already implements any functionality this project needs, with priority to the most widely used and recognised crates.
  2. **Open-source databases**: the approaches of the most recognised databases for the relevant task or functionality, such as Memgraph, Neo4j, PostgreSQL, InnoDB, MariaDB and MySQL.
  3. **Other open-source projects**: any project that reveals a relevant strategy, architecture or approach.
- Follow this decision order for every code and architecture decision:
  1. **Learn first.** The first approach must never reinvent the wheel. Make an informed decision by studying the references above and identifying **only the best** approaches of each project, then implement them originally in this project, or copy them where [Copying code](#open-source-references) allows.
  2. **Improve second.** Only when that process reveals a measurable improvement opportunity (see [Honest empiricism](#honest-empiricism-mandatory)), proceed to improve on the chosen approach.
- Treat source code as the most reliable source of truth. Whenever a reference project's code needs study, clone its public repository into a local folder under `/tmp/` (for example, `/tmp/<project>`), or reuse an existing clone there, and read the code directly. Use it for study, or to copy as [Copying code](#open-source-references) allows.

### Research and decision-making (modus operandi)

Apply this procedure to every research need and every technical decision:

1. **Study the right references.** For each decision, study together:
   - the leading open-source projects in that specific area, meaning the recognised reference for that exact problem; and
   - the Rust crates that already implement exactly that functionality, stating for each one whether it is used as a dependency or only studied.

   Skip projects that are not references for that area.
2. **Decide from patterns, not experiments.** Base each decision on the approaches that recur across the references and on their documented reasons. Do not run extensive tests to reach a decision. Measurement of the resulting code follows [Laboratory development](#laboratory-development-primary-principle-for-code).
3. **Decide autonomously.** When a decision grounded in the references satisfies the project's requirements, take it and record it with its evidence. Do not ask the user.
4. **Ask only on gaps or conflicts.** Ask the user only when the information is incomplete, conflicting or contradictory. Name the requirements or statements involved and offer two to four options, with the recommended option first.
5. **Be assertive.** State the decision, its evidence and its justification. Omit digressions.

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
