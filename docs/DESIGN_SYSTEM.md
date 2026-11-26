# gitRUST Design System

The native workbench organizes repository context, navigation, and inspection around a continuous workspace. Native window decorations provide window controls and compositor integration. The interface uses flat surfaces, muted dividers, and small corner radii. Copper identifies the active view and primary action. Blue identifies keyboard focus and text selection. Neutral surfaces distinguish hover and press. Green and red identify additions and removals, reinforced by symbols and line numbers.

## Visual direction

| Direction | Evaluation |
| --- | --- |
| Cool slate and blue | Familiar and readable, but visually close to the GitHub Desktop reference and uses blue for too many roles. |
| Graphite and copper | Selected. Warm accents identify gitRUST in the chrome, with blue reserved for focus and text selection and semantic colors reserved for Git information. |
| High-contrast terminal | Dense and keyboard-friendly, but monospace throughout makes headings and longer help text harder to distinguish. |

The original mark is drawn with egui lines and circles. Lucide and Phosphor were considered for their consistent icon families. The small set of controls fits a handful of first-party painter paths. Icons use a shared 16 pt size and either accompany text or provide an accessible name and tooltip. [Lucide licensing](https://lucide.dev/license) · [Phosphor Icons](https://phosphoricons.com/)

## Workflow references

| Reference | Observation and design decision |
| --- | --- |
| [GitHub Desktop](https://github.com/desktop/desktop) | The attached reference shows Changes and History next to the inspection area and gives context to empty states. gitRUST uses the same familiar vocabulary in its own chrome and keeps the empty workspace centered on the actions currently available. |
| [GitKraken](https://www.gitkraken.com/features) | Repository and branch context help orient graph-heavy workflows. The Stage 01 workbench places that context in the toolbar and leaves the commit graph to the history stage. |
| [Fork](https://git-fork.com/) | Prominent file and commit inspection makes it easy to scan across views. The selected row and the inspection heading use a common visual hierarchy. |
| [Sublime Merge](https://www.sublimemerge.com/) | Dense code review and a strong keyboard workflow suit long diffs. Line gutters, hunk headers and visible focus are included. Syntax highlighting remains a separate decision. |
| [GitButler](https://docs.gitbutler.com/) | Explicit branch and workspace context supports parallel lines of work. gitRUST shows an active branch and keeps GitButler's virtual-branch workflow outside the design-system scope. |
| [Lazygit](https://github.com/jesseduffield/lazygit) | Clearly separated panes and keyboard movement are useful under constrained space. The workbench uses Up, Down, Home and End in virtual lists and keeps the same actions in its narrow layout. |
| [Gitr](https://github.com/islandspan-solutions/gitr) | An egui Git graph viewer offers a compact native comparison point. gitRUST focuses this stage on the linear list, file selection, and diff readability. |
| [SourceGit](https://github.com/sourcegit-scm/sourcegit) | A cross-platform reference for explicit file, branch, and history categories. The Stage 01 views use a subset suited to a small window. |
| [Rerun](https://rerun.io/docs/getting-started/configure-the-viewer/navigating-the-viewer) | The egui viewer illustrates stable, resizable panels. gitRUST uses one resizable workspace pane and switches to list and detail screens when width is constrained. |

## Tokens

`crates/gitrust-ui/src/theme.rs` defines the typed palette, Git status colors, geometry and font roles. Values are logical points. System theme follows the window integration when it reports a theme. Explicit Light and Dark select the corresponding styles at runtime.

| Role | Dark | Light |
| --- | --- | --- |
| Background / surface | `#181A1E` / `#1E2126` | `#FBFAF8` / `#F2F1EF` |
| Elevated / selected | `#272B31` / `#342C29` | `#E9E7E4` / `#F2DFD4` |
| Primary / secondary | `#F0EEEB` / `#B4B5BE` | `#25252A` / `#54515C` |
| Accent / focus | `#F3AD86` / `#9CC7FF` | `#893D21` / `#245DA5` |
| Added line / removed line | `#202F29` / `#35272A` | `#E3F1E7` / `#F7E5E4` |

The geometry scale uses 4, 8, 12, 16 and 24 pt gaps and insets, 5 pt small corners and 1 pt strokes. Commands start at 28 pt tall and virtual list rows use 34 pt. The default list pane is 252 pt and the single-pane layout activates below 590 pt of content width. The system scale and the session zoom combine through egui.

Source Sans 3 provides window branding (19 pt), section headings (23 pt), body (15 pt), control labels (14 pt) and secondary text (13 pt). The Semibold face emphasizes headings and active navigation. Source Code Pro (13 pt) provides stable spacing for code, hashes, line numbers and diff text. Static TTFs preserve predictable local loading and accented glyphs.

Inter's screen-oriented proportions, IBM Plex Sans and Mono's matching family, Noto Sans's language coverage, and JetBrains Mono's code-oriented shapes were considered. Source Sans 3 and Source Code Pro provide matching UI and code faces with local static files and clear small-size character distinctions. Assets and license terms are listed in [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md). [Inter](https://github.com/rsms/inter) · [IBM Plex](https://github.com/IBM/plex) · [Noto Sans](https://github.com/notofonts/latin-greek-cyrillic) · [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)

## Component catalog

| Component | Composition |
| --- | --- |
| Button, IconButton, Toggle, Checkbox, RadioButton | Use egui interaction and disabled states. Use the primary button helper for the main action. The icon button adds a name and visible outline. |
| TextField, SearchField, TextArea | Compose native TextEdit using stable IDs and accessible names. Search includes an explicit clear control. |
| Dropdown, Menu, ContextMenu, Tabs, Tooltip | Use the framework popup, menu and tooltip behaviors. Selected tabs add a copper surface and rule. Hover and press use neutral surfaces, and a blue outline indicates focus. Context controls show an icon, caption and value. Unavailable context is displayed as status text. |
| Badge, StatusIndicator, Separator | Use semantic text, a symbol or a dot where appropriate, and the matching theme color. |
| ResizablePanel, Toolbar, StatusBar | Use the common shell frames with native window decoration. |
| ListRow, FileRow, BranchRow, CommitRow, ScrollableList | Supply the shared row model to `ListSelection::show`. The list lays out visible rows, uses fixture identity for persistent selection and focus, and surfaces the full name, secondary information and Git status to AccessKit. Fixed-width marks keep names aligned. |
| EmptyState, LoadingState, ErrorState, ProgressIndicator, Notification | Use the empty, loading and notice helpers. A spinner is reserved for the explicit loading example. |
| Modal, ConfirmationDialog | Compose an egui modal with the shared surface and actionable dismissal. Confirmation is exercised on a local sample draft. |

For a new component, start with the egui widget and theme styles. Add a wrapper when it provides a shared role, repeated composition, or accessibility behavior. Give stateful controls an ID based on their data and purpose. Verify keyboard focus, translation length, the smallest layout, and the color pairs the component actually renders. Add the interactive sample to the component gallery.

The default workspace reflects the current availability of Git integration. The diagnostic gallery intentionally supplies synthetic files, commits, branches, errors and progress examples. Start it with `gitrust --diagnostics` and open the synthetic workspace from the repository selector or the View menu. Additional launch examples are in [UI_ARCHITECTURE.md](UI_ARCHITECTURE.md).

The Stage 01.1 findings and native comparison images are indexed in the [design system audit](audits/DESIGN_SYSTEM_AUDIT.md).
