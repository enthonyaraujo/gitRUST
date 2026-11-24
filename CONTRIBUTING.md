# Contributing to gitRUST

Thank you for your interest in contributing to **gitRUST**. Our goal is to build a secure, fast, maintainable, and genuinely native open-source Git desktop client.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## Before Starting Significant Work

1. Review [`docs/ROADMAP.md`](docs/ROADMAP.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) to confirm your change aligns with the current project stage.
2. Check [`docs/adr/README.md`](docs/adr/README.md) for existing architectural decisions.
3. Open an issue to discuss major features, new dependencies, or architectural changes before submitting large pull requests.
4. Read [`AGENTS.md`](AGENTS.md) and [`docs/SECURITY_ARCHITECTURE.md`](docs/SECURITY_ARCHITECTURE.md) for non-negotiable technical and security invariants.

---

## Development Workflow

### 1. Environment Setup

Follow [`docs/BUILD.md`](docs/BUILD.md) to install the stable Rust toolchain (`rustc >= 1.95`) and platform graphics headers.

### 2. Branch Naming

Use descriptive topic branches:

- `feat/<short-topic>` — new stage capabilities
- `fix/<short-topic>` — bug fixes
- `docs/<short-topic>` — documentation or ADR updates
- `ci/<short-topic>` — continuous integration updates
- `refactor/<short-topic>` — internal structural improvements without behavior changes

### 3. Code Quality Standards

- **Language:** Write all code, identifiers, comments, tests, and documentation in English.
- **No Web Stack:** Do not add WebView, HTML/CSS/JS, or web-target crates.
- **Zero Unsafe:** Workspace crates enforce `unsafe_code = "forbid"`.
- **Error Handling:** Avoid `.unwrap()` and `.expect()` in library and application paths (`clippy::unwrap_used` is enabled). Return typed `Result` errors with clear context.
- **Dependencies:** Do not add crates for trivial tasks covered by `std`. Any new dependency must pass `deny.toml` license rules and be justified in the pull request description.
- **Honest UI:** Never merge placeholder buttons or mock views pretending to perform unimplemented operations.

### 4. Local Verification Checklist

Every pull request must pass all workspace checks locally without warnings:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

If `cargo-deny` is installed locally, also run:

```bash
cargo deny check
```

### 5. Testing Guidelines

- Place unit tests alongside modules (`#[cfg(test)]`) and cross-crate integration tests in `crates/<crate>/tests/`.
- For future Git tests (Stage 02+), create isolated temporary directories (`std::env::temp_dir()` / `tempfile`). **Never** run mutating or destructive Git tests against the `gitRUST` repository itself or any real user repository.
- Headless UI behavior should be tested via `egui::Context::run_ui`, verifying layout stability and ensuring idle frames do not schedule continuous repaints.

---

## Architecture Decision Records (ADRs)

Any change that alters crate boundaries, introduces major dependencies, modifies the Git execution strategy, or updates the security model requires a new ADR (or an ADR superseding an earlier one) under [`docs/adr/`](docs/adr/).

---

## Licensing of Contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in gitRUST shall be licensed under the [MIT License](LICENSE), without any additional terms or conditions.
