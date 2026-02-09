# Contributing to Fiddlesticks

Thanks for contributing. This document describes the recommended workflow for changes in this workspace.

## Scope and Stability

- `fiddlesticks` is the semver-stable public API boundary.
- Workspace crates (`fcommon`, `fprovider`, `ftooling`, `fchat`, `fmemory`, `fharness`, `fobserve`) are internal building blocks and may evolve more quickly.
- Prefer adding and stabilizing user-facing API through `fiddlesticks` unless low-level crate work is explicitly required.

## Prerequisites

- Rust `1.93` or newer (MSRV policy in `README.md`).
- A working local toolchain with `cargo fmt`, `cargo check`, and `cargo test`.

## Branching Model

- Use branch names in these patterns where possible:
  - `feature/<short-description>`
  - `fix/<short-description>`
- Open pull requests that target `prerelease`.
- `prerelease` is the integration branch for the next minor release.

## Development Workflow

1. Create a branch from `prerelease`.
2. Implement your change in focused commits.
3. Run local validation from workspace root:

```bash
cargo fmt --all
cargo check --workspace --all-features
cargo test --workspace --all-features
```

4. Update docs when behavior, features, or workflows change (`README.md`, crate README files, and this guide as needed).
5. Open a PR to `prerelease` with a clear description of what changed and why.

## Pull Request Expectations

- Keep PRs focused and reviewable.
- Include tests for behavior changes and regressions.
- Avoid unrelated refactors in the same PR.
- Call out breaking changes clearly, especially if they affect `fiddlesticks`.
- If you change feature flags, verify combinations still compile.

## Commit Message Recommendations

- Use short, imperative messages (for example: `add anthropic streaming retry coverage`).
- Explain intent in the body when context is not obvious.
- Keep each commit logically coherent.

## Security Reporting

- For security issues, follow `SECURITY.md`.
- Use GitHub Issues with the `security` label and include reproduction details.
- Do not include secrets or credentials in issue reports.

## Release Notes and Changelog Hygiene

- If your change affects users, include release-note-ready context in the PR description.
- Document migration notes for behavior changes that may impact downstream code.

## Code of Conduct

By participating, you agree to collaborate respectfully and constructively during review and discussion.
