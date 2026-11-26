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
- The Stage 02 inspection commands grant no transport access. Stage 03 uses separate typed provisioning commands and an HTTPS-only clone policy.

### 3.3 Information Disclosure (Credentials & Logs)

**Threat:** Personal access tokens, passwords, SSH passphrases, or private URLs with embedded credentials leak into config files, crash reports, or terminal logs.

**Controls:**
- **Declarative Baseline Defined in Stage 00 (`gitrust-storage`, `gitrust-providers`), Runtime Enforcement Planned for Stages 03/06/08:** `StorageSecurityBaseline` and `TlsVerificationPolicy` codify the prohibition on plaintext credential files, default telemetry, and TLS verification bypasses as compile-time constants (`no HTTP/storage I/O exists in Stage 00`). `tracing` logs in `gitrust-app` record only non-sensitive metadata (version, OS, graphics adapter, viewport metrics) and never dump environment variables.
- **Stage 03:** Accept only public HTTPS sources and reject URL user information. The local profile holds preferences and workspace paths.
- **Planned for Stages 06 and 08:** Delegate transport credentials to approved Git credential helpers or SSH agents, and store provider tokens in the OS keychain (`Secret Service`, `Windows Credential Manager`, or `macOS Keychain`). Omit URL user information from diagnostic output.

### 3.4 Spoofing & Man-in-the-Middle (Transport Security)

**Threat:** Compromised networks intercept HTTPS remote operations or hosting API calls.

**Controls:**
- **Implemented in Stage 00 (`gitrust-providers`):** `TlsVerificationPolicy::StrictCertificateVerification` is codified as mandatory.
- **Stage 03:** Force TLS verification for the public HTTPS clone. The same verification requirement applies to future provider and transport operations.

### 3.5 Denial of Service & Repository Corruption

**Threat:** Concurrent mutating Git commands corrupt `.git/index` or leave stale `.git/index.lock` files; huge untracked directories or binary diffs exhaust UI memory; accidental destructive commands erase uncommitted work.

**Controls:**
- The Stage 02 scheduler reserves exclusive access for repository write classes by canonical common Git directory. Stage 03 provisioning operates on a separate scratch directory and publishes to a new destination.
- Stage 02 bounds stdout, stderr, queued requests, task time and decoded items. Unix process groups and Windows Job Objects support termination of active reads. Stage 04 can use the typed models for virtualized rendering.
- Explicit confirmation gates for `DestructiveWrite` operations; automated destructive tests restricted strictly to ephemeral temporary directories (never real repositories).

---

## 4. Supply-Chain and Memory Safety Controls

- **Zero Workspace `unsafe`:** `[workspace.lints.rust] unsafe_code = "forbid"` prevents accidental memory-unsafety in first-party crates.
- **Minimal Dependency Footprint:** No web engines, scripting runtimes, or redundant utility crates. `eframe` is configured with `default-features = false` to exclude web targets (`web_screen_reader`).
- **Automated Auditing:** `deny.toml` and GitHub Actions (`EmbarkStudios/cargo-deny-action`) verify permissive MIT-compatible licenses, reject yanked crates, forbid wildcard version specs, restrict registries to `crates.io`, and check RustSec advisories on every pull request.


## Stage 03 provisioning and profile

The native application is a session trust coordinator. It stores no credentials, tokens or approval state in its profile. Local inspections preserve the validated-config allowlist, Git ownership checks and disabled network policy. The only LFS-related metadata accepted is the inert local repository-format marker with value zero. Filter configuration remains prohibited.

The explicit clone request uses HTTPS with TLS verification, cleared environment, credential helpers disabled, and redirects refused. Git can contact only the configured HTTPS source for this operation. Initializing and cloning use empty templates, private staging, and no-replace publication. The local materializer enables HFS and NTFS protections and writes symlink contents as regular files. Network permissions do not carry over to validation or materialization. Checkout filters and nested submodules are not activated. The engine depends on a trusted executable and a stable caller-controlled destination parent. [ADR-0008](adr/0008-workspace-management.md)

URL credentials, queries and fragments are rejected at the input boundary. Clone failures expose the exit status and discard raw process streams. A successful clone retains the accepted source as Git's local origin, so submit only addresses intended to be retained. The app cannot recognize a secret encoded as an opaque URL path. The profile keeps no clone URL and the workspace view receives only an endpoint-presence flag.

The no-replace guarantee applies when a competing entry appears at the final destination, including a symlink where supported. The caller controls the parent and its ancestors for the operation's lifetime. Paths are not held by directory handles on every platform, so replacing an ancestor concurrently remains outside the threat boundary. Cancellation after publication preserves the completed repository and its success result.

On Windows, publication uses the audited `tempfile` Windows backend to request `MoveFileExW` with neither replacement nor cross-volume copying. The private staging directory owns cleanup. The native CI test creates an empty destination after validation and checks that publication preserves both entries.
