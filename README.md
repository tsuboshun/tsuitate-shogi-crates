## Tsuitate Shogi Crates

The crates in this repository implement an engine for Shogi variants with special rules.
The following three types of variants are supported:

- Board sizes of 9×9 or smaller
- Tsuitate Shogi
- Dobutsu Shogi

For high-performance use cases, FFI interfaces can be provided to integrate the engine with programs written in other languages. Currently, only WebAssembly bindings are included, implemented under `tsuitate_bind`. Please refer to `sample.html` for instructions on how to use `tsuitate_bind`.

## Acknowledgements

This project builds upon and extends the [Rust shogi crates](https://github.com/rust-shogi-crates).