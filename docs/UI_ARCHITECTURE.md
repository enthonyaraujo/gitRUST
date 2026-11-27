# Native UI architecture

The binary owns eframe, wgpu, platform diagnostics, and native window lifecycle. `gitrust-ui` owns presentation and depends on egui and gitrust-core. It returns stack-allocated lifecycle events and delegates desktop actions to the coordinator. Core stays independent of egui. The UI crate performs no repository discovery, file I/O, network I/O or Git subprocess execution.

| Module | Responsibility |
| --- | --- |
| `theme` | Light/Dark palettes, semantic Git colors, dimensions, font roles and embedded font registration |
| `components` | Shared widget compositions, keyboard-aware virtual lists and shell primitives |
| `shell` | Workspace, selection, responsive layout, preferences and dialogs |
| `shell::toolbar` | Repository and branch selectors, direct fetch and publish entry points |
| `shell::commands` | Localized desktop menu order, key bindings, availability and action routing |
| `diff` | Visual alignment and indexing, unified/split rendering, inline changed spans and copy |
| `demo` | Read-only sample data and its presentation adapter |
| `gallery` | Diagnostic component pages, state scenarios and stress datasets |
| `i18n` | Typed English and Brazilian Portuguese message keys |

## Workspace and shell

The decorated window retains native move, resize and system controls. The in-window app menu follows the File, Edit, View, Repository, Branch, Help convention and collapses into one menu at narrow widths. The menus are rendered in egui on each desktop platform. The file preview combines navigation and file identity in the compact header to preserve room for the diff. Integration with the native macOS global menu bar is deferred.

An open repository shows the repository and branch selectors followed by direct fetch or publish. Remote settings, pull and push are available in the Repository menu. The View menu opens the GitHub workspace and the File menu opens application options. Menus and keyboard shortcuts open the same reviewed flows. File manager, terminal, editor, browser and diagnostic log actions are handled outside rendering. The Linux terminal and editor shortcuts use installed native applications from known executable paths. The Help log action exports the current window and graphics diagnostics to the application data directory.

Changes and History share a resizable sidebar that starts at 250 logical points. File checkboxes reflect staging in the Git index. The composer avatar opens author and signing controls, and co-authors are added as editable trailers in the commit description. The Changes composer has a fixed submit control while its fields and the file list scroll separately. Short file panels group stage, unstage and discard under Index actions. At very small logical viewport sizes, single-pane tabs keep the list, diff and composer reachable. The clean worktree lists available editor, file manager and GitHub actions in the main pane. Menus, selectors and dialogs constrain their content to the viewport, including at 200% scale. The existing palette, typography and accessible widget labels apply throughout.

The regular workspace starts empty with Options available. Diagnostic mode exposes the gallery and sample workspace and clearly identifies sample data. The diagnostic composer edits only its in-memory draft. Interactive launches persist theme, locale and scale as local preferences.

```sh
gitrust
gitrust --diagnostics
gitrust --diagnostic-view unified --theme dark
gitrust --diagnostic-view split --theme light --language pt-BR
gitrust --diagnostic-view history --scale 125
gitrust --diagnostic-view compose
gitrust --diagnostic-view branch
gitrust --diagnostic-view repository
gitrust --diagnostic-view long-diff
gitrust --diagnostic-view binary
gitrust --diagnostic-view stress-list
gitrust --diagnostic-view stress-diff
gitrust --diagnostic-view settings --smoke-size 640x400 --smoke-screenshot /tmp/gitrust-settings.ppm
```

## Rendering and list models

Use `ListSelection::show` with an item count, a position-to-key function and a row getter for that key. It receives only the range egui lays out. The widget ID derives from the list scope and stable item key, independent of filtering and display locale. On a keyboard navigation boundary it moves the viewport to the target index. The demonstration workspace recomputes filter matches only when the query or backing view changes. The gallery caches generated labels and adds new labels only when a larger dataset is first requested.

`TextDiff` stores a prepared, aligned list of visual lines and indexes unified lines at construction. The renderer can access a visible row by index in either mode. Paired lines highlight the segment between their common prefix and suffix using UTF-8 boundaries. Both modes use a standard two-axis scroll area. Split mode uses the same horizontal offset for both panes and keeps their gutters and hunk headers pinned. The viewport converts its vertical bounds to a visible row range. The stress diff derives each requested row from its index. Binary files and empty diffs have separate states.

Diagnostic views display static fixture text. Repository workspaces display worker-indexed Git patches and preserve line ownership, ordering and hunk alignment. The UI layer has no Git parser, commit-graph builder or syntax-highlighting dependency. Native drag selection is limited to rendered labels. The explicit Copy entire diff command returns the full unified body in either layout. It preserves source Unicode and tabs. The rendered view expands tabs at four-column stops, and native text selection copies displayed text. The sample body has no repository file headers.

## Stage 01.2 verification

Tab activation and keyboard shortcuts use the same shell transition. The transition preserves the current search query and recomputes the matching rows for the next tab. Row focus and selection continue to use the fixture identity within the list scope. Branch changes keep their separate reset behavior. Tabs publish the selected property on their AccessKit nodes while keyboard focus remains an independent state.

The existing `repository` and `branch` diagnostic views open the actual popup controls on launch. Native capture records include the executable digest, launch arguments, framebuffer dimensions and effective content points. The Stage 01.2 artifact set includes captures of both popups open and separate XWayland automation that activates their accessible controls. A smoke screenshot alone does not verify a subsequent action. Capture manifest (`target/stage01.2-evidence/capture-manifest.json`) · Interaction log (`target/stage01.2-evidence/native-access-final.log`)


## Stage 03 repository workspace

Interactive launches show the real repository workspace, with a searchable Repository selector, recent entries, and forms for open, init and clone. The UI emits repository actions and receives snapshots. Directory dialogs, Git commands, persistence and watcher management run outside rendering. The theme, typography, native AccessKit interactions and Wayland IME flow are shared with the existing shell. Synthetic workspaces remain available via diagnostics flags. [Repository management](REPOSITORY_MANAGEMENT.md)


## Stage 04 Changes workspace

The active workspace uses the Stage 01 diff viewport for worker-indexed real patches. Rows are keyed by path and comparison layer, and the coordinator ties results to a version of the authorized repository. File and diff rows are virtualized. The same controls back the compact tabs and the wide layout. The compositor holds editable text locally until a mutation succeeds and clears the submitted draft only when that commit is confirmed. Discard and amend show native confirmation dialogs. The AccessKit tree exposes file selection, hunk and line selection, the diff viewport and labeled commit fields.

[Changes and Commits](CHANGES_AND_COMMITS.md)


## Stage 05 branches and history

The Branch selector groups the default branch, local branches and locally available remote-tracking refs. It keeps native reference identity separate from display labels. Changes and History share the resizable sidebar and tab strip. History virtualizes graph-aligned commit rows and opens the first commit in the loaded snapshot. The selected commit occupies a full row and the branch comparison opens from the top of the list. The adjacent options menu exposes filtering and all branches, while Ctrl+F focuses the filter. At wide sizes the inspector shows a compact commit header with line totals, a changed-file column and a diff. Diff layout and copy controls share the file path header. The first file previews on demand. Compact tabs keep the history list, metadata and diff reachable at high zoom.

[Branches and History](BRANCHES_AND_HISTORY.md) · [ADR 0010](adr/0010-branches-history-and-merge.md)

## Stage 06 remote operations

The native Remote selector displays the selected endpoint and the upstream, with divergence labeled as local tracking data. Modal forms review endpoint, destination branch and strategy before a request is submitted. HTTPS secrets use a masked text field and move out of the form at submission. In-flight progress and cancellation use workspace events. Merge and rebase conflicts return to the existing Changes workspace. Remote labels have English and Brazilian Portuguese variants and use the existing egui accessibility bridge. [Remote operations](REMOTE_OPERATIONS.md)

## Stage 07 GitHub workspace

Accounts, repositories and pull requests use egui controls and provider snapshots without credential material. Explicit user actions start provider work. Native browser and file-picker actions are separate from the rendering loop. [GitHub integration](GITHUB_INTEGRATION.md)

## Stage 07.1 browser authorization

Welcome and Accounts share native OAuth controls. The pending dialog shows progress, browser reopening and cancellation. The coordinator receives typed progress from the provider worker and requests viewport focus after callback receipt. The signed-in account row identifies the method and opens its corresponding management flow. Device Flow is explicit under Accounts.
