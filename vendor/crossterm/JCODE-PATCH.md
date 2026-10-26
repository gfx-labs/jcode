# Jcode crossterm patch

This directory vendors the existing crossterm 0.29.0 dependency from crates.io.
The upstream MIT license is in `LICENSE`.
Registry archive checksum: `d8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b`.

The source, manifests, examples, README and changelog were copied from the Cargo
registry package. No dependencies were added. Local changes are limited to:

- `Cargo.toml`: declare a standalone workspace so the vendored tests can run.
- `README.md` and `examples/README.md`: remove inherited trailing whitespace.

- `src/event.rs`: expose the Unix terminal-query API and internal reply event.
- `src/event/terminal_query.rs`: run a bounded query using the shared event reader.
- `src/event/read.rs`: discard late replies without losing queued user events.
- `src/event/source/unix.rs`: include the shared Unix parser.
- `src/event/source/unix/{mio,tty}.rs`: use that parser and its Escape deadline.
- `src/event/source/unix/parser.rs`: recognize complete image probe replies and
  tmux passthrough envelopes before key decoding, preserve paste contents and
  unmatched input, and cover the recorded reply stream at every read split.
  CSI input is framed through its final byte before decoding; unsupported CSI
  sequences are discarded as a unit instead of exposing their suffix as keys.
  X10 mouse, Linux function keys and bracketed paste retain their payload parsing.

A lone Escape or ambiguous Alt-key prefix waits up to 100ms for a continuation.
Once a reply prefix is recognized, its bytes remain buffered across reads,
including after the query's absolute deadline, with a two-second idle deadline
that returns incomplete input to normal decoding. Candidate buffers are bounded
at 1024 bytes. Input is never
flushed, and no stdin reader thread is spawned. The query must run before an
`EventStream` is created. Windows event handling is unchanged by this patch.

To compare local changes with an installed registry copy, use `diff -ru` against
the exact package directory reported by `cargo metadata` before applying the
workspace patch. To run the owner-boundary regression:

```sh
cargo test --manifest-path vendor/crossterm/Cargo.toml --lib event::source::unix::parser::tests
```
