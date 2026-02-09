# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Added

- Introduced a facade-owned dynamic runtime builder: `fiddlesticks::AgentHarnessBuilder`.
- Introduced stable provider setup helpers: `build_provider_from_api_key`, `build_provider_with_config`, `list_models_with_api_key`.
- Added stable namespace modules for consumer imports: `fiddlesticks::chat`, `fiddlesticks::harness`, `fiddlesticks::memory`, `fiddlesticks::provider`, and `fiddlesticks::tooling`.

### Changed

- Removed deprecated `unstable-reexports` and legacy whole-crate facade re-exports.
- Nullhat harness integration now depends only on stable facade namespaces and helpers (no adapter-path imports).

### Migration

- Replace crate re-export imports:
  - `fiddlesticks::fchat` -> `fiddlesticks::chat`
  - `fiddlesticks::fharness` -> `fiddlesticks::harness`
  - `fiddlesticks::fmemory` -> `fiddlesticks::memory`
  - `fiddlesticks::fprovider` -> `fiddlesticks::provider`
  - `fiddlesticks::ftooling` -> `fiddlesticks::tooling`
- Replace direct adapter construction (`fprovider::adapters::*`) with facade setup APIs:
  - use `ProviderBuildConfig` + `build_provider_with_config`
  - use `list_models_with_api_key` for facade-level model listing
- Prefer `AgentHarnessBuilder` when creating dynamic, purpose-specific harness runtimes.
