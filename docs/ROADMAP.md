# Official Development Roadmap

This document defines the staged engineering plan for **gitRUST**. Each stage has explicit scope boundaries and quality gates; features are never presented as implemented prior to passing their stage verification criteria.

---

## Stage Overview

| Stage | Name | Status | Included in `v0.1.0` |
| --- | --- | --- | --- |
| **00** | Project Foundation | Completed and integrated | Yes |
| **01** | Design System | Completed | Yes |
| **02** | Git Engine | Completed and integrated | Yes |
| **03** | Repository Management | Planned | Yes |
| **04** | Changes and Commits | Planned | Yes |
| **05** | Branches and History | Planned | Yes |
| **06** | Remote Operations | Planned | Yes |
| **07** | Advanced Git Operations | Planned | Post-`v0.1.0` |
| **08** | Provider Integrations | Planned | Post-`v0.1.0` |
| **09** | Security and Performance | Planned (Continuous + Dedicated Audit) | Post-`v0.1.0` |
| **10** | Distribution and Release | Planned | Post-`v0.1.0` |

---

## Detailed Stage Breakdown

### Stage 00 — Project Foundation
- Cargo Workspace structure (`gitrust-app`, `gitrust-core`, `gitrust-ui`, `gitrust-git`, `gitrust-storage`, `gitrust-providers`) with acyclic dependencies and `unsafe_code = "forbid"`.
- Minimal compilable desktop shell using `egui` + `eframe` + `wgpu` with Wayland/X11 support and reactive idle rendering.
- Governance files, preliminary security architecture, benchmark methodology, ADRs `0001`–`0005`, and multi-OS CI validation.

### Stage 01 — Design System
- Original graphite/copper visual system, embedded typography, light/dark/system appearance, English and Brazilian Portuguese, and session zoom.
- Native application shell with opt-in gallery, synthetic repository preview, Changes and History workspaces, and unified/split diff foundations.
- Reusable egui components, semantic accessibility labels, resizable workspace, keyboard navigation, and virtual lists.
- [Design System](DESIGN_SYSTEM.md), [UI architecture](UI_ARCHITECTURE.md), and [accessibility scope](ACCESSIBILITY.md).

### Stage 02 — Git Engine
- Local repository discovery, trust gating and typed, shell-free inspection with system Git 2.45 or newer.
- Status, two-way diff, paged history and local/remote-tracking branches preserve byte-oriented paths and support SHA-1 and SHA-256.
- Bounded background concurrency, write reservations by common Git directory, read cancellation, structured errors, temporary-repository tests and opt-in benchmarks.
- Integrated into develop with the implementation commits preserved. [Git Engine contract and validation](GIT_ENGINE.md#stage-02-validation) · [PR #4](https://github.com/enthonyaraujo/gitRUST/pull/4)

### Stage 03 — Repository Management
- Add existing local repositories, initialize new repositories (`git init`), and clone remote repositories (`git clone` with progress reporting).
- Local workspace state and recent-repository persistence in `gitrust-storage` (XDG Base Directory on Linux, Known Folders on Windows, Application Support on macOS).
- Filesystem change detection / focus-refresh policy without excessive CPU wakeups.

### Stage 04 — Changes and Commits
- Working tree and index inspection (staged, unstaged, untracked, conflicted, and renamed files).
- Unified and split diff viewer with virtualized rendering for large diffs and binary file indicators.
- File-level and hunk/line-level staging/unstaging, discard changes (with explicit destructive-action confirmation), commit authoring, amend support, and commit signature compatibility.

### Stage 05 — Branches and History
- Local and remote-tracking branch listing, creation, switching, renaming, and safe deletion.
- Paginated/virtualized commit history list and commit detail inspector (changed files, author/committer metadata, parent links, diffs).
- Basic merge workflows and clear conflict state detection.

### Stage 06 — Remote Operations
- Remote configuration inspection (`origin` and additional remotes).
- `fetch`, `pull` (merge/rebase respecting repository config), and `push` with non-blocking progress feedback.
- Compatibility with system SSH agents and native Git credential helpers without storing plaintext credentials.

---

## First Public Milestone: `v0.1.0`

The first public release, **`v0.1.0`**, is planned to encompass **Stages 00 through 06**.

Advancing to `v0.1.0` requires:
1. Functional completion of Stages `00`–`06` without mock or partial placeholders.
2. Passing automated test suites and CI checks across Linux, Windows, and macOS.
3. Verification of the security invariants defined in [`docs/SECURITY_ARCHITECTURE.md`](SECURITY_ARCHITECTURE.md) (no shell execution, explicit repository trust and local inspection policy, no plaintext secrets, strict TLS).
4. Meeting stability and responsiveness baselines on real repositories.

---

## Post-`v0.1.0` Stages

### Stage 07 — Advanced Git Operations
- Interactive/guided stash management, cherry-pick, revert, tag management, rebase workflows, conflict resolution helpers, submodule/worktree awareness, and Git LFS status visibility.

### Stage 08 — Provider Integrations
- Optional, user-authorized integrations in `gitrust-providers` for GitHub, GitLab, and compatible self-hosted instances (pull/merge request discovery, clone-from-account, status checks).
- Native OS keychain integration for provider tokens with strict TLS verification.

### Stage 09 — Security and Performance
- Comprehensive threat-model review, fuzzing of Git output parsers (`cargo-fuzz` / property-based tests), memory/CPU profiling on large monorepos, and targeted evaluation of `gitoxide` (`gix`) for read-intensive hotspots where justified by measurements.

### Stage 10 — Distribution and Release
- Reproducible release pipelines, code signing, SBOM/provenance generation, Linux desktop integration (`.desktop`, AppStream metainfo, Flatpak / distro packaging artifacts), Windows installer, and macOS app bundle.
