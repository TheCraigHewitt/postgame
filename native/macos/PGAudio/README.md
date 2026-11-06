# PGAudio

The only Swift in postgame. It wraps the macOS APIs that have no good Rust
binding and exposes them through a small `@_cdecl` C ABI. `pg-capture` links it
as a static library through `swift-rs`. Use plain C types only at the boundary.
