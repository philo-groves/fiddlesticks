# Changelog

All notable changes to this project after 1.0.0 will be documented in this file.

The format is based on Keep a Changelog and this project follows Semantic Versioning for the `fiddlesticks` facade crate.

## Entry Template (copy for new releases)

```md
## [X.Y.Z] - YYYY-MM-DD

### Added
- 

### Changed
- 

### Fixed
- 

### Deprecated
- 

### Removed
- 

### Security
- 

### Migration Notes
- None.
```

## [2.0.0] - 2026-02-19

### Added
- Added first-class Ollama support across `fprovider` and `fiddlesticks`.
- Added `provider-ollama` feature flags in `fprovider` and `fiddlesticks`.
- Added Ollama adapter over the OpenAI-compatible transport with a default local base URL.
- Added Ollama model listing support through the facade provider helpers.

### Changed
- Added `ProviderId::Ollama` as a new provider enum variant.
- Extended provider parsing and macro shorthands to support `ollama` and `local` aliases.

### Migration Notes

- `ProviderId` now includes `Ollama`; exhaustive `match` statements over `ProviderId` must add a new `Ollama` arm.

## [1.0.0] - Not yet release

Changelog for 1.0.1 will appear here.

### Migration Notes

- None.
