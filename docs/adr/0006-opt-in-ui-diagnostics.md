# ADR-0006: Keep synthetic data in opt-in UI diagnostics

## Status

Proposed for Stage 01 review.

## Context

The native shell and visual components need realistic fixtures and stress datasets before the application can read Git repositories. The application also needs an honest default workspace.

## Decision

Enable the gallery and synthetic repository explicitly through the diagnostic CLI options. Keep fixture data and gallery state in `gitrust-ui`. Keep theme, language, zoom, selection and draft text in the window's presentation state. Preserve `gitrust-core` for lifecycle events and leave persistence to the storage stage.

The demonstration workspace exercises the shared shell, controls, and diff renderer. Diagnostic state never grants a Git operation or adds a filesystem, credential, or network dependency. Keep diagnostics available in a release build when explicitly requested so QA can inspect the same optimized rendering path.

## Alternatives

- Showing fixtures in the default workspace would make the visual workflow immediately discoverable, but could lead people to mistake sample data for repository state.
- A standalone demo binary would enforce separation by process, but would require maintaining a second bootstrap and could diverge from real window and graphics behavior.

## Consequences

QA can reproduce exact diagnostic views and compare launch modes on the native build. The source includes fixture data, which is constructed only while entering diagnostic views that use it. Preference persistence and Git-model adapters remain future integrations.
