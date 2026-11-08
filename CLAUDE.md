# rusbbo: notes for agents

- The Java server (`Havana-Server` in the Java Havana checkout, `$HAVANA_DIR`, by default `../aeyra-havana`) is the specification. When the Rust code and a trace disagree,
  the Rust code is wrong.
- Never edit files in `traces/` by hand, and never loosen a trace to make a check pass. Traces are
  only produced by `rusbbo-trace record` against the Java oracle (see `README.md`).
- Before calling work done: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, and, for server work, `rusbbo-trace check` on the traces the change touches.
- New behaviour needs a scenario first: write it in `scenarios/`, record it against the oracle, check
  that it passes against the oracle twice in a row, then implement.
- `crates/protocol/src/ids.rs` is generated: run `python3 tools/gen_message_ids.py` instead of editing it.
- Bytes on the wire are ISO-8859-1; keep bodies as `Vec<u8>`, not `String`.
