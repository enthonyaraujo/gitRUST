# ADR-0003: Use Git CLI Initially Behind an Abstract Engine Boundary

## Status

Accepted

## Context

A desktop Git client must never corrupt user repositories and must respect real-world developer environments: `.gitconfig` (`includeIf`), SSH agents, GPG/SSH commit signing, credential helpers, Git LFS, worktrees, submodules, and non-UTF-8 filenames. This matches the proven operational model of GitHub Desktop (which wraps Git via `dugite`) while keeping `gitRUST` open to pure-Rust acceleration in the future.

## Decision Drivers

- Repository integrity and 100% behavioral fidelity with official Git (`git/git`).
- Out-of-the-box compatibility with user SSH/GPG/credential/LFS configurations.
- Strong security boundary against shell injection and untrusted `.git/config` execution.
- Clean architectural seam allowing future evaluation or selective adoption of `gitoxide` (`gix`) without rewriting core state or UI layers.

## Considered Options

### Option 1: System Git CLI via Safe Rust Abstraction First, `gitoxide` Evaluable Later (Selected)
- **Pros:** Complete compatibility with official Git porcelain/plumbing, locks, hooks, signing, SSH, and LFS; zero C FFI memory-safety exposure in our process; clear adapter seam in `gitrust-git` enables benchmarking `gix` later for read-heavy paths.
- **Cons:** Requires `git` to be installed on the host (or bundled in future distribution stages) and incurs subprocess spawn overhead (~1–5 ms on Linux, ~10–25 ms on Windows) per command.

### Option 2: `git2` (`libgit2` C bindings) as Primary Engine
- **Pros:** In-process C library without subprocess creation overhead.
- **Cons:** Large C + OpenSSL/libssh2 FFI surface (`unsafe`), historically incomplete parity with modern Git features (sparse-index, SSH commit signing, partial clone, complex `.gitconfig` includes, LFS filters).

### Option 3: `gitoxide` (`gix`) Exclusively from Day One
- **Pros:** Pure Rust, fast in-process plumbing/status inspection.
- **Cons:** While `gix` (`0.90.x`) has mature read/clone/status building blocks, complex mutating porcelain workflows (interactive/3-way merges, rebase, GPG/SSH signing parity, arbitrary filter drivers, credential helper edge cases) still evolve rapidly pre-1.0. Relying solely on `gix` for mutations today risks subtle behavioral divergence from `git`.

## Decision

1. Use the **system `git` executable** as the primary engine starting in **Stage 02**, encapsulated strictly inside `gitrust-git`.
2. Enforce mandatory invocation invariants (already seeded in Stage 00's `GitCommandSpec`):
   - Direct process execution with discrete `OsString` arguments—**never** via a shell.
   - Full `PathBuf` / `OsStr` / byte-stream (`-z` NUL-delimited) support for non-UTF-8 paths.
   - Sanitized environment (`GIT_TERMINAL_PROMPT=0`, safe `-c` overrides on untrusted read paths) and `--` / `--end-of-options` argument separation.
   - Asynchronous background execution with cancellation support and a per-repository write queue/mutex for mutating operations.
   - Explicit confirmation gates for `DestructiveWrite` operations.
3. Define `gitrust-git` interfaces in domain terms (status snapshots, diff models, commit records) rather than leaking raw CLI flags into `gitrust-core` or `gitrust-ui`, enabling evidence-based `gitoxide` evaluation in **Stage 09**.

## Consequences

### Positive
- Maximum repository safety and seamless interoperability with existing developer Git setups.
- Zero C FFI dependencies (`libgit2`/`libssh2`) in `gitRUST`.
- Future `gix` adoption can proceed incrementally on read-only hotspots backed by `criterion` benchmarks and differential tests against `git`.

### Negative / Risks
- Windows process creation latency requires batching plumbing queries (e.g., `git status --porcelain=v2 --branch -z` in one invocation instead of multiple small commands) during Stage 02 design.

## Related Decisions

- [ADR-0001](0001-rust-as-primary-language.md)
- [ADR-0004](0004-cargo-workspace-organization.md)
