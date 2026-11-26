# Branches and History

Open an authorized local workspace. The Branch selector shows local branches and locally available remote-tracking branches. Select a row to see its configured upstream and ahead/behind counts when Git can compute them from local objects. Remote-tracking refs are updated by Git outside this application during this stage.

## Branch operations

- Create a branch from HEAD, a selected local or remote-tracking branch, or a commit in History. A branch created from a remote-tracking reference configures tracking against the existing local remote configuration.
- Switch with Git's safe checkout. Local staged and unstaged changes can move with the checkout when compatible. Git refuses changes that would overwrite work, including ignored files. A branch already open in another linked worktree remains protected.
- Rename a branch, including the current branch, without replacing an existing name. Deletion requires confirmation and uses Git's safe branch deletion after confirming that the tip is reachable from the current HEAD.
- An unborn branch has no commit to copy. Make the first commit or switch to an existing local branch when available.

## History and the commit inspector

**HEAD** walks the current commit and its ancestors. **All branches** also walks the locally available branch tips, including remote-tracking refs. Each page uses a fixed set of object IDs and topological order. New locally observed tips invalidate the corresponding snapshot. Use **Load more commits** for the next page. Searches match subject, author or hash among already loaded commits.

Graph lanes continue between pages. The view shows nine lanes in the gutter and marks nodes beyond that width with an ellipsis. Filtering preserves commit rows but suppresses inter-row connectors. Each snapshot loads up to 8,000 commits for this view.

Open a commit to read its full object ID, message, author and committer, times with recorded offsets, parents, changed files and line counts. **Copy hash** copies the complete ID. Open a parent to navigate there, or select it as the base of a merge commit comparison. Root commits are compared with the empty tree for their object format. Choose any changed file and view its Unified or Split diff. Signature labels describe the signature header and its format. The application does not verify signatures in this stage.

## Merge and conflicts

Select a local source branch and choose **Merge into current branch**. Review the source and attached destination. The merge requires a clean index and worktree, and Git protects ignored files. An already integrated source is reported directly. A fast-forward advances the branch. A three-way merge pauses to review the result and create the merge commit.

For a conflict, open Changes, inspect the affected files and resolve them in an external editor. Explicitly stage each resolution. Once the index contains no unresolved entries, use **Finish merge**. The merge commit uses the current index and both parents. Author information and explicit OpenPGP or SSH signing follow the [Changes and Commits policy](CHANGES_AND_COMMITS.md).

**Abort merge** requires confirmation and an in-session proof held by the workspace that started the merge from a clean state. The engine compares the current Git status, merge source and exact affected file bytes. Later edits, staging, an external or restarted merge, or an unsupported file type can make abort unavailable. Conflicting edits remain in the worktree. Git's locks and normal checks govern changes made by independent Git processes. Application cancellation triggers a workspace refresh so completed effects remain visible.

[ADR 0010](adr/0010-branches-history-and-merge.md)

## Desktop branch commands

Local comparison reports commits unique to each side using captured HEAD and reference tips. The worker validates those tips before comparing; the command uses no network. Updating from the default branch opens the normal merge review for its local counterpart. A known upstream or origin symbolic HEAD takes precedence over the conventional local main or master names.

Squash uses Git to stage the combined tree and leaves commit creation to the existing composer. Conflict resolutions remain in Changes. The local non-interactive rebase uses the same safe continuation and abort proof as remote rebase and guards against rewriting signed commits or merge commits. Stash saves staged, unstaged and untracked files while keeping ignored files in place. View lists bounded local stash history and opens the selected stash commit in the existing inspector. Restore and drop remain advanced stash operations.
