# archscan

Architectural health linter for JS/TS codebases — written in Rust.

Detects circular dependencies between files by parsing imports (via tree-sitter)
and analyzing the resulting dependency graph with Tarjan's SCC algorithm.

## Build

```bash
cargo build --release
```

> Note: this environment ships Rust 1.75 via apt, so `Cargo.lock` pins a few
> transitive dependencies (indexmap, clap, ignore, globset) to versions that
> don't require the `edition2024` Cargo feature. On a machine with a recent
> Rust toolchain (via rustup) you can safely run `cargo update` to get the
> latest compatible versions of everything.

## Usage

```bash
archscan analyze /path/to/project
archscan analyze /path/to/project --format json
```

## Roadmap (see project plan)

- Milestone 2: more detectors (dead code, god classes, layer violations)
- Milestone 3: SARIF output for GitHub Code Scanning integration
- Milestone 4: additional language support (Python, Go, Rust) via more
  tree-sitter grammars — the parser module is already structured to make
  this a matter of adding a grammar, not rearchitecting.
