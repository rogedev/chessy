# Chessy

> A native chess GUI built with Rust, egui, and Stockfish.

<p align="center">
  <img src="screenshot.png" width="600" alt="Chessy screenshot" />
</p>

---

## Features

- Native desktop UI powered by [egui](https://github.com/emilk/egui)
- [Stockfish](https://stockfishchess.org/) engine integration with multi-line evaluation
- Evaluation panel with score, depth, and suggested move sequences
- PGN import/export
- Move history navigation

## Requirements

| Requirement | Notes |
|---|---|
| Rust (2024 edition) | Install via [rustup](https://rustup.rs/) |
| Stockfish | Must be available in `$PATH` — [download](https://stockfishchess.org/download/) |

## Build & Run

```sh
cargo run --release
```

## License

[LICENSE](LICENSE)
