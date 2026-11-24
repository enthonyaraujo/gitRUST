# Security Policy

Security is a foundational priority for **gitRUST**. Because a desktop Git client routinely interacts with untrusted repositories, SSH/HTTPS remotes, and authentication credentials, security controls are designed into the workspace from Stage 00.

For architectural details on threat boundaries and mitigations, see [`docs/SECURITY_ARCHITECTURE.md`](docs/SECURITY_ARCHITECTURE.md).

---

## Supported Versions

gitRUST is currently in pre-release development (`Stage 00 — Project Foundation`, pre-`v0.1.0`). Security fixes are applied exclusively to the latest development branch (`main`) until the first public release (`v0.1.0`) is tagged.

| Version | Supported |
| --- | --- |
| `main` (pre-`v0.1.0`) | Yes |
| `< 0.1.0` snapshots | Latest commit on `main` only |

---

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues, discussions, or pull requests.**

Instead, use **GitHub Private Vulnerability Reporting** on the repository:

1. Navigate to the **Security** tab of the [`gitRUST` repository](https://github.com/enthonyaraujo/gitRUST/security).
2. Click **Report a vulnerability** to open a private security advisory draft.
3. Provide:
   - Affected commit hash or version (`gitrust --version`).
   - Operating system, display server (if relevant), and `git --version`.
   - A minimal, non-destructive proof of concept or reproduction steps.
   - Potential impact assessment (for example, arbitrary command execution via a crafted repository, credential exposure, or path traversal).

### Response Targets

As an open-source project in active early development, maintainers aim to:

- Acknowledge receipt within **5 business days**.
- Provide an initial triage and remediation plan within **14 days**.
- Coordinate public disclosure after a fix is merged and verified.

---

## In-Scope Security Areas

- Command or argument injection when invoking external binaries (`git`, credential helpers, SSH).
- Arbitrary code execution triggered by opening, inspecting, or diffing a crafted/malicious Git repository (e.g., via `.git/config`, hooks, filters, `fsmonitor`, or symlinks).
- Exposure of credentials, tokens, SSH keys, or sensitive environment variables on disk or in logs.
- Transport security bypasses (such as weakened or disabled TLS verification).
- Path traversal or unintended file writes outside an intended target directory.
- Memory-safety defects or soundness issues in dependencies.

## Out of Scope

- Vulnerabilities residing solely in an outdated system `git` binary or OS kernel not caused by insecure invocation flags from gitRUST.
- Intentional execution of scripts explicitly initiated by the user outside gitRUST.
- Denial-of-service caused solely by opening multi-terabyte files on systems without sufficient physical memory, where no memory unsafety or data corruption occurs.
