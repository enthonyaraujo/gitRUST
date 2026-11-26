# Remote operations

Open and authorize the workspace, then use **Remote** in the toolbar or the **Repository** menu. The initial selection comes from the current upstream, then `origin`, then the first configured remote. Selecting a remote or opening the menu reads local data. The selector shows fetch and push endpoints, the upstream, and divergence against local tracking refs. A successful fetch records its UTC time for the active workspace.

## Configure remotes

Add, edit, rename, or confirm removal of a remote. A blank push URL uses the fetch URL. Set the upstream by entering the remote branch name, for example `main`. Each operation revalidates the repository and the configuration shown at confirmation. Remotes with custom fetch mappings require the Git CLI for rename or removal. Local configurations with an unsupported transport or multiple URLs for one direction show that direction as unavailable. An edit can replace that configuration with a supported endpoint.

## Fetch, pull and push

- **Fetch** sends an explicit request to the selected remote or the checked remotes. It updates and prunes branches within each selected remote's tracking namespace. Other remotes, tags and local branches keep their refs. Batch fetch stops on the first failure and records the remotes that completed.
- **Pull** requires an attached branch, a configured upstream and a clean worktree. The confirmation shows the destination branch and locally known divergence. Choose Fast-forward, Merge or Rebase. A divergent fast-forward-only operation keeps the fetched refs and reports that integration requires a different strategy. A fast-forward updates the branch, while an already integrated tip returns an up-to-date outcome.
- **Merge pull** uses the Changes workspace for any pending merge. Resolve files externally, stage the resolutions in Changes and finish the merge with the regular author and signing controls.
- **Rebase pull** rewrites a linear chain of unsigned commits. When conflicts arise, resolve them externally, stage them in Changes and select **Continue rebase**. **Abort rebase** requires confirmation and an unchanged conflict snapshot. If recovery controls are unavailable because the operation was started externally, the session was restarted or the proof no longer matches, inspect the Git state and recover with the Git CLI. The app leaves those files and metadata in place.
- **Push** sends the confirmed branch tip to the explicit destination branch. Use **Publish branch** for the first push and opt into setting upstream after success. The server can reject the operation, including for a non-fast-forward update, branch protection or missing permissions. The app reports a disconnected or cancelled push as uncertain if it cannot establish the server's result. Push preserves the objects and signatures sent to the server. A fetch refreshes tracking data when local refs have yet to reflect a push, including with separate fetch and push endpoints.

Progress has a phase and a percentage when Git provides both. Other phases show an indeterminate state. Transfers run on a bounded worker, use a supervisor timeout and can be cancelled. After an uncertain push, confirm the state with an explicit fetch when connectivity returns.

[Authentication and SSH trust](AUTHENTICATION.md) · [ADR-0011](adr/0011-remote-operations.md)
