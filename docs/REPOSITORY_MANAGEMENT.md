# Repository management

The interactive desktop opens with **Add local repository**, **Create**, **Clone**, and any saved recent workspaces. The Repository selector searches names and paths. Use **Remove** to forget an entry or **Show in file manager** to reveal its directory. Removing an entry changes the catalog and never deletes repository data.

## Open and trust

Type a path or choose a folder in the native picker. A subdirectory resolves to its containing repository. Review the canonical path and decide whether the app can inspect its local metadata. Normal worktrees, supported linked worktrees and bare repositories follow the Git Engine layout policy. Each worktree has its own canonical catalog key even when the engine shares a scheduling identity for linked worktrees.

Authorization belongs to the current process. On restart the app identifies the last successfully opened workspace and prompts for authorization. Opening any recent entry also requires authorization again. A rejected or missing workspace leaves the previous usable workspace available, where applicable. A missing entry remains in recents until it is reopened or removed. An open workspace shows its checkout state and a basic summary of staged, modified, untracked and conflicted paths. Bare repositories show checkout information without a worktree summary.

## Create and clone

Choose an existing parent and a new directory name. Creation suggests the initial branch `main`. Clone suggests a directory from the public HTTPS URL. The confirmation buttons create or clone only after the user submits the form. Both operations reject an occupied destination, including an empty existing directory.

Provisioning runs in a private temporary sibling. Init uses an empty template. Clone uses HTTPS, refuses embedded credentials, query strings, fragments and redirects, and verifies TLS using the Git installation's trust roots. It obtains metadata without checkout and checks it against the local engine policy. An internal local-only operation materializes the checked tree with symlinks represented as files, filesystem filename protections enabled and submodules excluded. The result is validated before publication. After publication the user reviews the final path and can authorize it for the session.

Git reports percentages for individual transfer phases. While a percentage is unavailable the app shows activity without estimating a number. Cancellation waits for child termination and scratch cleanup. If it arrives after publication, the completed workspace is offered for authorization. Destination files that already exist have priority. Choose a parent controlled by the local user. OS or filesystem errors can prevent scratch cleanup.

Use public repository URLs intended for sharing. Credential helpers, OAuth, SSH, private remotes, Git LFS materialization, shallow clones, partial clones, remote management, and recursive submodule checkout are outside this stage.

The activity area keeps the requested destination separate from the active workspace. A successful operation shows the published path in the trust dialog. During inspection, the existing summary is explicitly marked as the last completed result. The toolbar reads only whether local configuration has an endpoint. Remote operations remain unavailable in this stage.

## Local files

The profile uses platform config and local application-data directories. Linux uses XDG, Windows uses the roaming configuration and local data Known Folders, and macOS uses Application Support. A platform-native filename remains lossless in storage.

Each file starts with `GTRS`, one byte of format version (1), one platform byte (1 for Unix, 2 for Windows), and a kind byte. `preferences.bin` uses kind `P` and carries appearance and language bytes plus a little-endian 16-bit scale percentage. `recent.bin` uses kind `R` and carries a little-endian 16-bit count and last-selected index (65535 means none), followed by path length and native path bytes for each record. Unix stores opaque filename bytes and Windows stores little-endian UTF-16 code units. Files are capped at 1 MiB and paths at 16 KiB. The catalog is bounded to 64 workspaces and uses most-recent-first order.

The storage worker syncs a same-directory temporary file and atomically replaces the old file. On Unix it also syncs the parent directory. Corrupt or unreadable data is reported and safe defaults are used. The app writes a file only after a preference or catalog change and preserves unknown versions. The format has no trust, credentials, or telemetry fields. [ADR-0008](adr/0008-workspace-management.md)

## Refresh and shutdown

Only the active workspace has a native filesystem watcher. Events are grouped for 300 ms, with a two-second limit for sustained changes. Read/access notifications do not schedule inspections. A focus transition or manual refresh works independently of the watcher. The coordinator cancels superseded requests and checks task IDs before applying results. The GUI receives replacements for its latest pending snapshot and wakes on updates. Closing the app discards inactive watchers and shuts down the Git workers.
