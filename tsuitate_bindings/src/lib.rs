mod game_api;
#[cfg(feature = "python")]
mod py_game;
mod rl;
mod sfen_util;
mod wasm_game;

pub use game_api::GameApi;
pub use rl::{ActionHistory, FoulAttempt, move_to_action_index};

pub use wasm_game::{WasmGame, WasmInfo};

#[cfg(feature = "python")]
#[pyo3::pymodule]
fn tsuitate_bindings(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    py_game::register(m)
}
