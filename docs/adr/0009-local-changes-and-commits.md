# ADR 0009 — Local changes and commits

- Status: Accepted
- Scope: Local index and worktree mutations, commit creation, amend and signing

## Context

Workspace status represents two Git comparisons, and a single path can appear in both. Editing the index requires coordination with other gitRUST jobs and normal Git processes. Destructive actions must refer to the content shown during review. The Git floor is 2.45.0.

## Decision

- Use a typed background request with an authorized repository, a complete porcelain status snapshot and a mutation action. The coordinator associates previews and approvals with the authorized workspace version. Repository discovery also checks directory identity so replacing metadata at the same Unix path invalidates prior trust.
- Reserve the canonical common Git directory for each write. Hold the worktree's real `index.lock` and execute index updates against a private scratch index. Publish by syncing the lockfile and renaming it over the index. Use NUL-delimited literal operands for whole-file actions. Existing lockfiles and unselected index entries belong to their original owners.
- Keep raw Git patches inside the engine. Produce a separate immutable display projection, reconstruct selected edits from bytes, re-read the exact patch under the index lock and require Git's cached apply check. Mode/type changes, binary changes, and renames stay at whole-file granularity.
- Review destructive operations with exact per-file patches. Restore unstaged content from the index or restore staged-only content from HEAD. Delete approved untracked regular files through directory descriptors without following symlink ancestors.
- Construct commits with `write-tree` and `commit-tree`. Advance the local branch with `update-ref` using its expected object ID. Author information is explicit. Amend requires separate confirmation and retains the original author and parents. Only fixed, verified local signing executables may run, using an explicitly selected key. An existing signature cannot be silently removed.
- Preserve the allowlist, deadlines, process supervision and network-denying environment of the local engine. Block general-purpose Git execution, automatic hooks and implicit remote operations.

## Consequences

Index publication and branch compare-and-swap rely on Git's and the filesystem's individual guarantees. Index, refs and worktree are separate resources. An interruption during a multi-file restore can leave some files changed, and a failed commit can leave unreachable Git objects. The coordinator inspects the real state after mutations. Directory checks and content revalidation reduce race windows with concurrent editors. The trust model requires the user to control write access to local metadata. Abrupt or adversarial external replacements cannot be treated as a supported concurrent transaction.

The release contract targets Linux for Stage 04. Portable abstractions remain in place, while native compatibility and filesystem behaviors on Windows and macOS remain Stage 07 work.

## References

- [git-apply](https://git-scm.com/docs/git-apply)
- [git-commit-tree](https://git-scm.com/docs/git-commit-tree)
- [git-update-ref](https://git-scm.com/docs/git-update-ref)
