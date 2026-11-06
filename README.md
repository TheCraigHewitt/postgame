# postgame

An open-source alternative to Gong: bot-free call capture, on-device
transcription and interaction metrics, stored locally. No bot joins your
meetings. Early-stage: this repo currently holds the macOS Phase 0 spike.

## Develop

Requires Apple Silicon, macOS 14.4+, Xcode, Rust (via rustup), pnpm and
`cargo-deny`.

    cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cargo deny check

## License

Licensed under Apache-2.0. Contributions require DCO sign-off (`git commit -s`).
