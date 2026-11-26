# Accessibility

gitRUST uses egui's native widgets and the eframe AccessKit bridge. Icon-only actions have text names and hover help. Search and draft inputs have accessible names, and visible preference labels are associated with their controls. Git status is conveyed with both a localized name and a symbol or a signed diff line. The two themes use a separate blue outline for focus. Active tabs and menu selections use copper.

## Keyboard operation

| Keys | Action |
| --- | --- |
| Tab / Shift+Tab | Move focus forward or back between current controls |
| Enter / Space | Activate the focused button through egui |
| Up / Down, Home / End | Move selection inside the focused visible list and scroll to its target |
| Ctrl+F / Cmd+F | Focus the search field in the demonstration workspace |
| F6 | Move focus to the open diff viewport |
| Left / Right, Up / Down, Page Up / Page Down | Scroll the focused diff viewport |
| Alt+1 / Alt+2 | Switch demonstration Changes or History |
| Ctrl+, / Cmd+, | Open Settings |
| Escape | Dismiss the active popup or dialog. In the narrow workspace, return from details to the list. |

Confirmation exposes Cancel and Confirm, and Escape dismisses the confirmation. Menus and dropdowns follow egui keyboard behavior. Standard controls preserve screen-reader roles. The virtual lists expose mounted rows. Diff gutters are painted. Mounted text exposes localized before and after line numbers via AccessKit. Offscreen rows enter the tree when scrolled into view.

## Validation scope

Headless tests exercise focus traversal into file rows, arrow selection, the Settings shortcut, AccessKit bounds for closing the localized dialog at narrow width, the visible local-draft notice, and line numbers in the mounted diff. The diff test checks that keyboard and wheel input move both text runs and preserve the gutter positions. Color tests use WCAG 2.2 AA numerical thresholds as a reference for text (4.5:1) and focus (3:1) on covered surfaces. This is a color and interaction baseline for native desktop UI. [WCAG 2.2](https://www.w3.org/TR/WCAG22/#contrast-minimum)

Native visual checks use the real Linux application. The XWayland input test focused the filter, entered the file list with Tab, changed files with Down, moved back with Shift+Tab, opened Settings and dismissed it with Escape. Enter and Space activated the focused tabs, while Alt+1 and Alt+2 switched tabs without modifying the focused filter. F6, repeated horizontal movement and repeated Page Down were exercised in XWayland on the long and 100,000-line diffs. Headless tests also exercise menu activation, splitter dragging, localized bounds and the disabled commit control.

Orca ran through a private AT-SPI session with process-local settings. Its speech log shows the filter entry and focused files with path and Git status. The row uses the platform selection property to convey selection independently of a checked state. The test covers the sample workspace on Linux and does not assert audible playback, a full screen-reader walkthrough, or assistive-technology behavior on Windows and macOS. Orca speech log (`target/stage01.1-evidence/orca-debug.log`)

## Stage 01.2 Orca and AccessKit

The rebuilt application ran under XWayland with Orca on a private AT-SPI bus and process-local settings. The driver used window-targeted X11 keystrokes and AT-SPI activation and focus actions. The four runs covered the filter and tab shortcuts, repository and branch actions, the Settings dialog, the draft fields and discard confirmation, and the Split diff scroll area.

- Changes and History exposed `page tab` roles and the correct selected state. Alt+2 and Alt+1 preserved the entered filter and updated the result count. Orca generated speech for the History tab, the populated filter, the clear action and the file row.
- Both selectors opened and their menu actions changed the application context. Orca generated menu and item speech. Settings exposed a dialog and focusable Close button. Escape dismissed it.
- Orca generated speech for the summary and description fields, the unavailable-commit explanation and the discard dialog. AT-SPI exposed Commit as disabled and insensitive. Cancel retained the draft and Confirm cleared the synthetic text. The sample did not establish an explicit Orca announcement of the disabled button.
- Orca generated speech for the Diff scroll area. Mounted before and after lines exposed line numbers and code in the AT-SPI tree. The run exercised F6 and horizontal movement. Reading the full diff through Orca and audible playback remain outside the sample.

The normal desktop accessibility and screen-reader settings remained unchanged. Physical touchpad gestures and other assistive technologies remain outside this Linux revalidation. Interaction log (`target/stage01.2-evidence/native-access-final.log`) · Orca log (`target/stage01.2-evidence/orca-debug.log`)

## Stage 01.3 Wayland text input

The Wayland feedback workaround runs after the widgets have handled preedit, including active ranges and empty preedit used to cancel an edit. If eframe runs a logic-only fallback while the window is focused, the previous editor output remains active until the widgets can draw again. IME activation, commits, native focus events, keyboard navigation and AccessKit remain enabled. Regression tests exercise deleting the last preedit character, confirming Unicode, cancelling an active preedit with an empty commit, the focused and unfocused redraw fallback, and focusing both draft fields in the normal compact composer.

The rebuilt application received real IBus Intelligent Pinyin preedits and Unicode commits through Mutter's Wayland text-input protocol. The native checks covered candidate selection after a deliberate pause, Backspace deleting the last composing character, Escape cancelling preedit, and AT-SPI exposing the edited values in the filter, both draft fields and all three gallery inputs. At 150% with the Portuguese locale, the description retained its composition across a compositor resize and accepted new preedit after native focus left and returned. IME run (`target/stage01.3-evidence/native-ime-final.log`) · Gallery multiline input (`target/stage01.3-evidence/native-ime-gallery-click.log`) · Resize and focus (`target/stage01.3-evidence/native-ime-resize.log`)

An XWayland ABNT2 check covered ordinary input, dead keys, Left, Delete, Backspace, selection and Unicode clipboard replacement in the filter, both draft fields and the three gallery inputs. The Wayland ABNT2 check completed that sequence in both draft fields. The Wayland check remained partial for the filter and the gallery editing sequence. XWayland keyboard (`target/stage01.3-evidence/native-keys-xwayland.log`) · Summary repeat (`target/stage01.3-evidence/native-keys-summary.log`) · Wayland keyboard (`target/stage01.3-evidence/native-keys-wayland-motion.log`)

Orca 51.1 generated speech for the filter and both draft fields on a private XWayland AT-SPI session. The test checked the selected tabs, local draft and discard confirmation and inspected the disabled Commit control in AT-SPI. Coverage is limited to the generated speech and observed tree and actions. Audible playback, other readers and other platforms remain outside this test. Orca speech (`target/stage01.3-evidence/orca-debug.log`) · Interaction checks (`target/stage01.3-evidence/orca-run.log`)
