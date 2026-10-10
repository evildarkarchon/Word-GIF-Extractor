# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.

## How a label is applied

This repo's issue tracker is GitHub Issues (`docs/agents/issue-tracker.md`). All five labels
exist on the repo, so apply them with `gh issue edit <n> --add-label "<label>"` — never create a
near-duplicate. An issue carries at most one triage label: re-triaging means removing the old one
and adding the new one in the same edit. An open issue with no triage label is untriaged and should
be read as `needs-triage`.

## Finishing work

When a ticket's work is implemented and committed, close the issue with
`gh issue close <n> --reason completed` and record where the work landed (commit, PR) in the
closing comment. A PR whose body says `Closes #<n>` does this on merge. Closing replaces the old
local `Status: done` — there is no `done` label, because a closed issue no longer shows up in the
open queue an agent picks work from.

A spec stays open until every one of its tickets is closed, then is closed itself.

`wontfix` issues are closed with `--reason "not planned"` and keep the `wontfix` label.
