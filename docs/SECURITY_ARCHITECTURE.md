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

**Stage 02 controls**

- Identify candidates without Git execution. Before inspection, require explicit authorization of stable local metadata and validate the layout, config and reference backend. Disable local config includes during preflight. Reject disallowed configuration, including content filters, which can apply to status and diff.
- Clear the subprocess environment and exclude system/global config and attributes, optional locks, replacement refs, pagers, prompts, external diff and text conversion. Preserve Git's ownership checks. The policy refuses external object stores, promisor repositories and unsupported Git directory indirection.
- Launch only the absolute native Git chosen from trusted PATH entries or by the caller. Disable transports and lazy fetch even when harmless remote metadata is present. Filesystem and executable provenance remain trust assumptions. [Git Engine contract](GIT_ENGINE.md)

### 3.2 Command and Argument Injection

**Threat:** Repository-controlled names, revisions, paths or configured extensions alter command interpretation.

**Stage 02 controls**

- The engine alone constructs the arguments of a closed set of Git operations. It uses native argument arrays, full object IDs, validated reference names and literal repository-relative paths. Use `--end-of-options` for supported revision commands and `--` for path or ref operands.
- No transport or mutating operation is available. Later stages must validate remote schemes and credential/transport behavior separately.

### 3.3 Information Disclosure (Credentials & Logs)

**Threat:** Personal access tokens, passwords, SSH passphrases, or private URLs with embedded credentials leak into config files, crash reports, or terminal logs.

**Controls:**
- **Declarative Baseline Defined in Stage 00 (`gitrust-storage`, `gitrust-providers`), Runtime Enforcement Planned for Stages 03/06/08:** `StorageSecurityBaseline` and `TlsVerificationPolicy` codify the prohibition on plaintext credential files, default telemetry, and TLS verification bypasses as compile-time constants (`no HTTP/storage I/O exists in Stage 00`). `tracing` logs in `gitrust-app` record only non-sensitive metadata (version, OS, graphics adapter, viewport metrics) and never dump environment variables.
- **Required in Stage 03 / Stage 06 / Stage 08:** Delegate Git transport credentials to the user's configured Git credential helper / SSH agent, and store any provider API tokens exclusively in the OS keychain (`Secret Service` on Linux, `Windows Credential Manager`, `macOS Keychain`). Strip `userinfo` (`https://user:token@host`) before logging or displaying remote URLs.

### 3.4 Spoofing & Man-in-the-Middle (Transport Security)

**Threat:** Compromised networks intercept HTTPS remote operations or hosting API calls.

**Controls:**
- **Implemented in Stage 00 (`gitrust-providers`):** `TlsVerificationPolicy::StrictCertificateVerification` is codified as mandatory.
- **Required in Stage 06 / Stage 08:** Never pass `-c http.sslVerify=false` to Git and never expose a "disable SSL verification" toggle for provider HTTP clients.

### 3.5 Denial of Service & Repository Corruption

**Threat:** Concurrent mutating Git commands corrupt `.git/index` or leave stale `.git/index.lock` files; huge untracked directories or binary diffs exhaust UI memory; accidental destructive commands erase uncommitted work.

**Controls:**
- The Stage 02 scheduler reserves exclusive access for both write classes by canonical common Git directory. The executable allowlist contains read operations only.
- Stage 02 bounds stdout, stderr, queued requests, task time and decoded items. Unix process groups and Windows Job Objects support termination of active reads. Stage 04 can use the typed models for virtualized rendering.
- Explicit confirmation gates for `DestructiveWrite` operations; automated destructive tests restricted strictly to ephemeral temporary directories (never real repositories).

---

## 4. Supply-Chain and Memory Safety Controls

- **Zero Workspace `unsafe`:** `[workspace.lints.rust] unsafe_code = "forbid"` prevents accidental memory-unsafety in first-party crates.
- **Minimal Dependency Footprint:** No web engines, scripting runtimes, or redundant utility crates. `eframe` is configured with `default-features = false` to exclude web targets (`web_screen_reader`).
- **Automated Auditing:** `deny.toml` and GitHub Actions (`EmbarkStudios/cargo-deny-action`) verify permissive MIT-compatible licenses, reject yanked crates, forbid wildcard version specs, restrict registries to `crates.io`, and check RustSec advisories on every pull request.
