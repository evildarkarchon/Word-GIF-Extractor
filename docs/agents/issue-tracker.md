# Issue tracker: Local Markdown

Issues and specs for this repo live as markdown files in `.scratch/`.

Do **not** use `gh issue` for tracking work. The repo still has a GitHub remote
(`evildarkarchon/Word-GIF-Extractor`), so `gh` remains available for PRs, releases, and
labels — but the issue queue the skills read and write is the local one described here.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec is `.scratch/<feature-slug>/spec.md`
- Implementation issues are one file per ticket at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01` — never a single combined tickets file
- Triage state is recorded as a `Status:` line near the top of each issue file (see `triage-labels.md` for the role strings)
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

`.scratch/` is **not** gitignored, so issue files are committed alongside the code they
describe. Add `.scratch/` to `.gitignore` if you'd rather keep them local-only.

## When a skill says "publish to the issue tracker"

Create a new file under `.scratch/<feature-slug>/` (creating the directory if needed).

## When a skill says "fetch the relevant ticket"

Read the file at the referenced path. The user will normally pass the path or the issue number directly.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a file with one **child** file per ticket.

- **Map**: `.scratch/<effort>/map.md` — the Notes / Decisions-so-far / Fog body.
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Wayfinder:` line records where the ticket sits on the map — `open`, `claimed`, or `resolved`, with an absent line read as `open`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `Wayfinder: resolved`.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are `Wayfinder: open` and unblocked; first by number wins.
- **Claim**: set `Wayfinder: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Wayfinder: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.

Wayfinding state gets its own `Wayfinder:` line rather than reusing `Status:`, because `Status:`
carries one of the five canonical triage roles and nothing else (`triage-labels.md`). The two lines
are independent: a wayfinder ticket may carry both, and claiming or resolving it never rewrites its
triage role.
