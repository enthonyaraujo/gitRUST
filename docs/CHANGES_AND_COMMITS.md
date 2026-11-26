# Changes and Commits

Open and authorize a local repository to inspect its Changes workspace. The file list shows staged, unstaged, untracked, and conflicted entries. When both the index and the working tree differ, a file appears on both layers. Each row retains its native path. The filter matches the displayed path or the source of a rename, and filtering preserves checked files.

## Inspect and stage changes

- Select a row for its diff. **Staged** compares HEAD with the index. **Unstaged** compares the index with the working tree. Untracked files are compared with an empty source. Conflicts show Git's combined diff.
- Switch between Unified and Split. Split scrolls both sides together. **Copy full diff** copies the selected file's entire textual patch, including Git's headers. The clipboard uses UTF-8 replacement characters for invalid sequences. The engine retains the original bytes for mutations.
- Check one or more file rows and choose **Stage** or **Unstage**. **Stage all** includes unstaged and untracked files regardless of the current filter. Resolving a conflict is an individual selection. **Unstage all** affects the entire index.
- For supported text changes, check hunks or individual additions and deletions in the diff. Use **Stage selection** or **Unstage selection** to apply only checked content. Either half of a changed line may be selected independently. This also works for nonempty text additions and deletions, including an unborn repository.
- Binary, rename, type and permission changes use whole-file operations. Combined conflicts have no partial controls. Git validates reconstructed patches before application. A selection that would put another line after an unterminated line is rejected.

At compact widths or high zoom, use the Files, Diff, and Commit tabs. A wider window shows the resizable sidebar, compositor and diff together. Tab moves between controls. F6 focuses the diff viewport for arrow-key and Page Up/Page Down scrolling. The tools use the existing Light, Dark and System palettes and English and Brazilian Portuguese labels.

## Confirm a discard

Choose **Review discard** to inspect the exact selected paths, their textual diffs and restoration sources. An unstaged discard restores the worktree copy from the index. A staged discard restores both copies from HEAD and is blocked if it would overwrite unstaged content. On an unborn branch, unstage a staged file first. Untracked regular files can be permanently deleted after explicit confirmation. This uses a descriptor-relative operation on Linux and macOS. Symlink traversal, ambiguous selections, conflicts and unsupported filesystem operations are rejected.

External edits invalidate a review. Git protects index and ref publication using lockfiles. A batch can still stop after changing some worktree files. The workspace refreshes after success, failure or cancellation so the current state can be inspected.

## Create or amend a commit

1. Stage the intended content and enter a title and optional description. The compositor shows the branch and staged count.
2. Set a name and e-mail under **Author and signing** if the repository has no local identity. The values apply to the current commit. The application leaves local and global Git configuration untouched.
3. Choose **Commit staged changes**. The commit uses only the index, including in repositories with no prior commits. If Git rejects the commit, the draft stays available.

**Load last commit** opens amend mode and loads the current message. Amend preserves the original author, author date and parents. It uses the current index, allowing both message-only amendments and amendments with new staged content. Confirm the rewrite in the dialog. If the branch tip moves after the message was loaded, reload before rewriting. Commit creation requires an attached or unborn branch and is blocked while a merge, rebase, cherry-pick or revert is in progress.

## Sign a commit

Linux supports the fixed system executables `/usr/bin/gpg` and `/usr/bin/ssh-keygen`. Their resolved paths must belong to root and cannot be writable by group or other users. Select a signer explicitly for each commit.

| Format | Input |
| --- | --- |
| OpenPGP | The full hexadecimal fingerprint of a key in the user's GPG keyring. The signing process can use an absolute `GNUPGHOME` provided at launch. |
| SSH | An absolute path to a local key usable by the system signer. Automatic SSH agent discovery is outside this stage. |

The application never reads private key material into its own state and never saves signer selections to the repository profile. An unavailable signer or unusable key yields an error. Amending a signed commit requires a new signature. Windows/macOS signing, protected key prompt behavior across desktop sessions, and native UI compatibility remain Stage 08 work.

See [ADR 0009](adr/0009-local-changes-and-commits.md) and [Git Engine](GIT_ENGINE.md#stage-04-mutations) for the execution contract.
