# postgame agent guide

postgame is an open-source (Apache-2.0) alternative to Gong. A desktop app
records the mic and the meeting app's audio as two separate tracks with no
bot, transcribes on-device, computes interaction metrics, and writes briefs
with the user's own LLM key. Everything is stored locally. Only transcript
text, never audio, goes to the provider the user chose.

`docs/` is local-only context and is gitignored. Before any task, read
`docs/SPIKE_PLAN.md`, which wins over the other docs for the spike. Never commit
`docs/`.

## Build and test

    cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cargo deny check
    pnpm -C apps/desktop tauri dev

Hardware tests are `#[ignore]`. Run them on a Mac with permissions granted:

    cargo test -p pg-capture -- --ignored

## Crate boundaries

- One task touches one crate plus its tests. Each crate README states its contract.
- Traits change only with a note in the crate README.
- `pg-analysis` stays pure: no I/O.
- All in-call times are `i64` milliseconds from the mic-track start.
- Swift lives only in `native/macos/PGAudio`, behind a small C ABI. Everything
  else is Rust.

## Rules

- Never commit audio, transcripts of real calls, `.db` files, keys or `docs/`.
- Tests never hit the network. LLM tests use recorded responses in
  `fixtures/llm/`. Live calls are gated by `PG_LIVE_LLM=1`.
- No new dependency without a license check (`cargo deny check`). Ask before
  adding anything with an unusual license.
- No code copied from GPL, AGPL or Screenpipe sources, or from Anarlog `enterprise/`.
- Describe Gong only from public docs.

## Workflow

- One milestone per branch: `spike/m<N>-<slug>`.
- Keep PRs under about 400 changed lines. Split if bigger.
- Each PR states its acceptance checklist and how it was tested.
- Sign off every commit: `git commit -s`.
