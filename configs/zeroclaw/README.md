# ZeroClaw config boundary

This directory is intentionally small.

- `zeroclaw` itself is treated as an external runtime and is not vendored into this repo.
- These files are placeholders for runtime/profile templates that can evolve separately from our Rust wrappers.
- The current wrappers only verify that this config boundary exists; they do not parse these files yet.

Expected future use:
- runtime profile selection
- browser/file tool defaults
- environment-specific overrides
