# Preliminary Security Architecture

This document defines the security model, trust boundaries, threat analysis, and mandatory engineering controls for **gitRUST** starting at Stage 00.

---

## 1. Security Philosophy

A desktop Git client operates at a high-value intersection: it parses untrusted repository contents from external collaborators, invokes local developer tools (`git`, SSH, GPG, credential helpers), and interacts with authenticated remote servers.

gitRUST adopts a **defense-in-depth, zero-trust-toward-repositories** posture:

1. **Every opened or cloned repository is treated as potentially untrusted input.**
2. **Memory safety is enforced without exceptions** (`unsafe_code = "forbid"` across workspace crates).
3. **Subprocesses never pass through command shells.**
4. **Secrets never touch gitRUST plaintext configuration or diagnostic logs.**
5. **Privacy is preserved by default** (zero telemetry, zero unsolicited network calls).

---

## 2. Trust Boundaries

```mermaid
flowchart LR
    subgraph Trusted["Local User & OS Boundary"]
        UI["gitrust-ui / gitrust-app"]
        Core["gitrust-core"]
        Keychain["OS Keychain / SSH Agent"]
    end

    subgraph Controlled["Controlled Execution Boundary"]
        GitCrate["gitrust-git (Sanitized argv & env)"]
        GitBin["System git Executable"]
    end

    subgraph Untrusted["Untrusted External Inputs"]
        RepoFS["Repository Working Tree & .git/config"]
        Remote["Remote Git Servers & Hosting APIs"]
    end

    UI --> Core
    Core --> GitCrate
    GitCrate -->|"Direct exec (no shell)"| GitBin
    GitBin <-->|"Guarded flags"| RepoFS
    GitBin <-->|"SSH / Strict TLS"| Remote
    GitBin <--> Keychain
```

---

## 3. STRIDE Threat Model & Controls

### 3.1 Tampering & Arbitrary Code Execution via Malicious Repositories

**Threat:** Opening or inspecting a crafted repository triggers arbitrary code execution via `.git/config` directives (`core.fsmonitor`, `core.sshCommand`, `core.pager`, `core.editor`, `diff.external`, clean/smudge filters) or `.git/hooks/*`.

**Controls (Stage 02+ Git Engine Gate):**
- Read-only inspection commands (`status`, `diff`, `log`, `branch`, `rev-list`) must pass safe configuration overrides (`-c core.fsmonitor=false`, `--no-optional-locks`, `--no-ext-diff`, `--no-textconv` unless explicitly trusted) and disable pager execution (`GIT_PAGER=cat` or `--no-pager`).
- Never execute files from `.git/hooks/` directly from `gitRUST`. When Git commands invoke user hooks (e.g., `commit`, `push`), surface hook presence clearly and allow safe `--no-verify` or trusted-repository scoping in Stage 09.
- Never execute binaries located relative to the current working directory on Windows (`Command` resolution must resolve a verified `git` executable path rather than relative current-directory lookup).

### 3.2 Command and Argument Injection

**Threat:** Crafted branch names, file paths (e.g., `-oProxyCommand=...`, `--upload-pack=...`), or remote URLs inject unintended flags into `git` or SSH subprocesses.

**Controls:**
- **Implemented in Stage 00 (`gitrust-git`):** `GitCommandSpec` enforces discrete `OsString` argument arrays and rejects embedded NUL bytes; shell invocation (`sh -c`, `cmd /C`) is architecturally absent.
- **Required in Stage 02+:** Always pass `--end-of-options` and `--` to separate flags from revisions and file paths; validate remote URL schemes (`https://`, `ssh://`, standard SCP-like `git@host:path`), rejecting dangerous `ext::` transport helpers by default (`GIT_ALLOW_PROTOCOL=https:ssh:file`).

### 3.3 Information Disclosure (Credentials & Logs)

**Threat:** Personal access tokens, passwords, SSH passphrases, or private URLs with embedded credentials leak into config files, crash reports, or terminal logs.

**Controls:**
- **Implemented in Stage 00 (`gitrust-storage`, `gitrust-app`):** `StorageSecurityBaseline` forbids plaintext credential persistence; `tracing` logs record only non-sensitive metadata (version, OS, graphics adapter, viewport metrics) and never dump environment variables or raw command arguments containing URLs/secrets.
- **Required in Stage 03 / Stage 06 / Stage 08:** Delegate Git transport credentials to the user's configured Git credential helper / SSH agent, and store any provider API tokens exclusively in the OS keychain (`Secret Service` on Linux, `Windows Credential Manager`, `macOS Keychain`). Strip `userinfo` (`https://user:token@host`) before logging or displaying remote URLs.

### 3.4 Spoofing & Man-in-the-Middle (Transport Security)

**Threat:** Compromised networks intercept HTTPS remote operations or hosting API calls.

**Controls:**
- **Implemented in Stage 00 (`gitrust-providers`):** `TlsVerificationPolicy::StrictCertificateVerification` is codified as mandatory.
- **Required in Stage 06 / Stage 08:** Never pass `-c http.sslVerify=false` to Git and never expose a "disable SSL verification" toggle for provider HTTP clients.

### 3.5 Denial of Service & Repository Corruption

**Threat:** Concurrent mutating Git commands corrupt `.git/index` or leave stale `.git/index.lock` files; huge untracked directories or binary diffs exhaust UI memory; accidental destructive commands erase uncommitted work.

**Controls:**
- Per-repository asynchronous write lock serializing all `SerializedWrite` and `DestructiveWrite` commands (Stage 02).
- Bounded output buffers and paginated/virtualized diff rendering (Stage 02 / Stage 04).
- Explicit confirmation gates for `DestructiveWrite` operations; automated destructive tests restricted strictly to ephemeral temporary directories (never real repositories).

---

## 4. Supply-Chain and Memory Safety Controls

- **Zero Workspace `unsafe`:** `[workspace.lints.rust] unsafe_code = "forbid"` prevents accidental memory-unsafety in first-party crates.
- **Minimal Dependency Footprint:** No web engines, scripting runtimes, or redundant utility crates. `eframe` is configured with `default-features = false` to exclude web targets (`web_screen_reader`).
- **Automated Auditing:** `deny.toml` and GitHub Actions (`EmbarkStudios/cargo-deny-action`) verify permissive MIT-compatible licenses, reject yanked crates, forbid wildcard version specs, restrict registries to `crates.io`, and check RustSec advisories on every pull request.
