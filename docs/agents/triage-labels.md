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

This repo's issue tracker is local markdown (`docs/agents/issue-tracker.md`), so there is
nothing to create up front — a label is just the role string written on the `Status:` line
near the top of the issue file:

```markdown
# Cover detection misses spine-only EPUBs

Status: needs-triage
```

Re-triaging means editing that line. A file with no `Status:` line is untriaged and should
be read as `needs-triage`.

The repo's GitHub labels are now unrelated to triage — `ready-for-agent` and `wontfix`
exist there from the previous GitHub-backed setup, but no skill reads them.
