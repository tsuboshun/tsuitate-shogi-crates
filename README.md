## Tsuitate Shogi Crates

The crates in this repository implement an engine for Shogi variants with special rules, with a particular focus on imperfect-information variants.
The following three types of variants are supported:

- Board sizes of 9×9 or smaller, including Dobutsu Shogi
- Tsuitate Shogi
- Dark Shogi

For high-performance use cases, FFI interfaces can be provided to integrate the engine with programs written in other languages. WebAssembly and Python bindings are implemented under `tsuitate_bindings`. Please refer to `sample.html` for instructions on how to use the WebAssembly bindings.

### Python bindings

For local development, prepare a Python environment and install the bindings in editable mode:

```sh
python -m venv .venv
source .venv/bin/activate
python -m pip install maturin pytest
cd tsuitate_bindings
python -m maturin develop
python -m pytest python_tests
```

To build an AWS Lambda Linux/arm64 wheel, install the cross-compilation prerequisites and run the build script from the repository root:

```sh
rustup target add aarch64-unknown-linux-gnu
python -m pip install maturin ziglang
./scripts/build_py_game_lambda_wheel.sh
```

The wheel is written to `target/wheels`.

## Acknowledgements

This project builds upon and extends the [Rust shogi crates](https://github.com/rust-shogi-crates).
