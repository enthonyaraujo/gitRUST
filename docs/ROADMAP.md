# Official Development Roadmap

This document defines the staged engineering plan for **gitRUST**. Each stage has explicit scope boundaries and quality gates; features are never presented as implemented prior to passing their stage verification criteria.

---

## Stage Overview

| Stage | Name | Status | Included in `v0.1.0` |
| --- | --- | --- | --- |
| **00** | Project Foundation | Completed and integrated | Yes |
| **01** | Design System | Completed | Yes |
| **02** | Git Engine | Completed and integrated | Yes |
| **03** | Repository Management | Completed on Linux and integrated | Yes |
| **04** | Changes and Commits | Completed on Linux and integrated | Yes |
| **05** | Branches and History | Integrated | Yes |
| **06** | Remote Operations | Integrated | Yes |
| **07** | GitHub Integration | Linux implementation under review | Yes |
| **08** | Cross-Platform Validation | Planned (release gate) | Yes |
| **09** | Advanced Git Operations | Planned | Post-`v0.1.0` |
| **10** | Additional Provider Integrations | Planned | Post-`v0.1.0` |
| **11** | Security and Performance | Planned (Continuous + Dedicated Audit) | Post-`v0.1.0` |
| **12** | Distribution and Release | Planned | Post-`v0.1.0` |

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
- Open, authorize, track, reopen and remove local workspace entries.
- Initialize new workspaces and clone public HTTPS repositories with progress, cancellation and secure publication.
- Persist appearance and workspace history and refresh the active workspace on filesystem changes, focus or manual request.
- Validated on Linux and integrated into `develop` at `dc9a6eb`. [Repository management](REPOSITORY_MANAGEMENT.md) · [PR #6](https://github.com/enthonyaraujo/gitRUST/pull/6)

### Stage 04 — Changes and Commits
- Working tree and index inspection (staged, unstaged, untracked, conflicted, and renamed files).
- Unified and split diff viewer with virtualized rendering for large diffs and binary file indicators.
- File-level and hunk/line-level staging and unstaging, reviewed discard, commit authoring, amend, and explicit OpenPGP and SSH signing. [Changes and Commits](CHANGES_AND_COMMITS.md) · [ADR 0009](adr/0009-local-changes-and-commits.md) · [Integration](https://github.com/enthonyaraujo/gitRUST/pull/10)

### Stage 05 — Branches and History
- Local and remote-tracking branch listing, creation, switching, renaming, and safe deletion.
- Paginated/virtualized commit history list and commit detail inspector (changed files, author/committer metadata, parent links, diffs).
- Basic merge workflows and clear conflict state detection. [Branches and History](BRANCHES_AND_HISTORY.md) · [ADR 0010](adr/0010-branches-history-and-merge.md) · [PR #11](https://github.com/enthonyaraujo/gitRUST/pull/11)

### Stage 06 — Remote Operations
- Manage remotes and upstreams and explicitly fetch, pull, publish or push.
- HTTPS and SSH clone and authentication on Linux, with routed progress, cancellation and reviewed conflict recovery. [Remote operations](REMOTE_OPERATIONS.md) · [Authentication](AUTHENTICATION.md) · [ADR-0011](adr/0011-remote-operations.md)

---

### Stage 07 — GitHub Integration
- GitHub App Device Flow, Linux Secret Service, repository exploration and account associations.
- GitHub clone and publication, forks, pull requests, checks and workflow status through the native UI. [GitHub integration](GITHUB_INTEGRATION.md) · [App setup](GITHUB_APP_SETUP.md)

### Stage 08 — Cross-Platform Validation
- Re-enable the Windows and macOS automatic CI matrix. Verify native tests and desktop behavior on Linux, Windows, and macOS.
- Recheck exclusive repository publication on Windows, the directory picker, graphics, filesystem notifications, signing integrations, and other portability issues against the completed MVP.
- The initial Stage 03 Windows CI failed when a concurrently created empty directory could be replaced. The correction passed native tests on the Stage 03 merge. Track the regression through the Stage 08 validation. [First failed job](https://github.com/enthonyaraujo/gitRUST/actions/runs/45836809755/job/139967749025) · [Stage 03 merge](https://github.com/enthonyaraujo/gitRUST/actions/runs/45848058298)

## First Public Milestone: `v0.1.0`

Stages 00 through 07 form the functional MVP. Stages 04 through 07 use Linux-first development and automatic CI for formatting and Clippy, MSRV, Linux build and tests, and dependency and license auditing. Stage 08 gates the first public release and any cross-platform support announcement.

Advancing to `v0.1.0` requires:
1. Functional completion of Stages `00`–`07` without mock or partial placeholders.
2. Passing automated test suites and CI checks across Linux, Windows, and macOS.
3. Verification of the security invariants defined in [`docs/SECURITY_ARCHITECTURE.md`](SECURITY_ARCHITECTURE.md) (no shell execution, explicit repository trust and local inspection policy, no plaintext secrets, strict TLS).
4. Meeting stability and responsiveness baselines on real repositories.

---

## Post-`v0.1.0` Stages

### Stage 09 — Advanced Git Operations
- Interactive/guided stash management, cherry-pick, revert, tag management, rebase workflows, conflict resolution helpers, submodule/worktree awareness, and Git LFS status visibility.

### Stage 10 — Additional Provider Integrations
- Optional, user-authorized integrations in `gitrust-providers` for GitLab, Bitbucket, and compatible self-hosted instances (pull/merge request discovery, clone-from-account, status checks).
- Native OS keychain integration for provider tokens with strict TLS verification.

### Stage 11 — Security and Performance
- Comprehensive threat-model review, fuzzing of Git output parsers (`cargo-fuzz` / property-based tests), memory/CPU profiling on large monorepos, and targeted evaluation of `gitoxide` (`gix`) for read-intensive hotspots where justified by measurements.

### Stage 12 — Distribution and Release
- Reproducible release pipelines, code signing, SBOM/provenance generation, Linux desktop integration (`.desktop`, AppStream metainfo, Flatpak / distro packaging artifacts), Windows installer, and macOS app bundle.
