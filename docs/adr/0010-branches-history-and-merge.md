# ADR 0010 — Branches, history, and merge

- Status: Proposed for Stage 05 review
- Scope: Local refs, bounded history views, commit comparisons, and two-head merges

## Decision

- Dispatch typed branch and merge mutations through the existing scheduler, exclusive by common Git directory. Revalidate authorization, the status snapshot, and selected ref IDs. Use Git's branch and checkout commands to enforce names, locking, and linked-worktree restrictions.
- Walk validated full object IDs with a fixed anchor and immutable branch tips when requested. Use topological pages, retain lane assignments across pages, and paint only visible rows. Compute added and removed lines separately from the full patch. Read a patch only when an inspector file is selected.
- Let Git own merge state. Use fast-forward where possible and pause three-way merges before commit. Disable autostash, automatic conflict reuse, merge hooks and automatic maintenance. Complete a single-source merge with a draft that uses the Stage 04 author and signing policy.
- Permit abort only with an opaque engine-created proof from a clean app-started merge. Retain affected file bytes up to the safety limit and check them and the porcelain status again at confirmation. Avoid aborting external merges and retain unsupported states for regular Git tooling.

## Consequences

Git and the filesystem supply their own lock guarantees. Worktree contents and references can change between separate operations by an independent process. The application refreshes after each terminal mutation, including cancellation and failure. The local boundary depends on user-controlled repository metadata. Cross-platform application validation stays in Stage 08.

- [Git switch](https://git-scm.com/docs/git-switch)
- [Git merge](https://git-scm.com/docs/git-merge)
- [Git branch](https://git-scm.com/docs/git-branch)
