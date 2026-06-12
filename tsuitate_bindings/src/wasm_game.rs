use shogi_core::Color;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use crate::game_api::GameApi;

#[cfg(target_arch = "wasm32")]
fn js_error(message: &str) -> JsValue {
    JsValue::from_str(message)
}

#[cfg(not(target_arch = "wasm32"))]
fn js_error(_message: &str) -> JsValue {
    JsValue::NULL
}

fn parse_optional_csa_color(csa_color: Option<char>) -> Result<Option<Color>, JsValue> {
    match csa_color {
        Some('+') => Ok(Some(Color::Black)),
        Some('-') => Ok(Some(Color::White)),
        Some(_) => Err(js_error("invalid color")),
        None => Ok(None),
    }
}

#[derive(Tsify, PartialEq, Eq, Clone, Debug)]
pub enum WasmInfo {
    None = 0,
    Foul = 1,
    FoulUnderCheck = 2,
    Check = 3,
    Checkmate = 4,
    LossByFoul = 5,
    Draw = 6,
}

#[wasm_bindgen]
pub struct WasmGame {
    inner: GameApi,
}

#[wasm_bindgen]
impl WasmGame {
    #[wasm_bindgen(constructor)]
    pub fn new(
        sfen: &str,
        game_kind: u8,
        is_tsuitate: bool,
        promotion_rank: u8,
        foul0: i8,
        foul1: i8,
        draw_move_count: u16,
        last_info: Option<u8>,
    ) -> Result<WasmGame, JsValue> {
        let inner = GameApi::new(
            sfen,
            game_kind,
            is_tsuitate,
            promotion_rank,
            foul0,
            foul1,
            draw_move_count,
            last_info,
        )
        .map_err(|err| js_error(&err.to_string()))?;
        Ok(Self { inner })
    }

    #[wasm_bindgen]
    pub fn make_move(&mut self, csa_move: &str) -> bool {
        self.inner.make_move(csa_move)
    }

    #[wasm_bindgen]
    pub fn make_move_ignore_turn(&mut self, csa_move: &str) -> bool {
        self.inner.make_move_ignore_turn(csa_move)
    }

    #[wasm_bindgen(getter)]
    pub fn sfen(&self) -> String {
        self.inner.sfen(None, false)
    }

    #[wasm_bindgen]
    pub fn sfen_for(
        &self,
        csa_color: Option<char>,
        is_dark_shogi: Option<bool>,
    ) -> Result<String, JsValue> {
        Ok(self.inner.sfen(
            parse_optional_csa_color(csa_color)?,
            is_dark_shogi.unwrap_or(false),
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn fouls(&self) -> Vec<i8> {
        self.inner.fouls()
    }

    #[wasm_bindgen]
    pub fn set_fouls(&mut self, foul0: i8, foul1: i8) {
        self.inner.set_fouls(foul0, foul1);
    }

    #[wasm_bindgen(getter)]
    pub fn last_info(&self) -> Option<u8> {
        self.inner.last_info()
    }

    #[wasm_bindgen(getter)]
    pub fn last_move(&self) -> Option<String> {
        self.inner.last_move_csa(None)
    }

    #[wasm_bindgen]
    pub fn last_move_for(&self, csa_color: Option<char>) -> Result<Option<String>, JsValue> {
        Ok(self
            .inner
            .last_move_csa(parse_optional_csa_color(csa_color)?))
    }

    #[wasm_bindgen(getter)]
    pub fn last_capture(&self) -> Option<String> {
        self.inner.last_capture()
    }

    #[wasm_bindgen]
    pub fn is_valid(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        self.inner.is_valid(csa_move, is_tsuitate)
    }

    #[wasm_bindgen]
    pub fn is_valid_ignore_turn(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        self.inner.is_valid_ignore_turn(csa_move, is_tsuitate)
    }

    #[wasm_bindgen]
    pub fn from_candidates(
        &self,
        csa_color: char,
        csa_from: &str,
        csa_piece: &str,
        is_tsuitate: bool,
        ignore_turn: bool,
    ) -> Vec<String> {
        self.inner
            .from_candidates(csa_color, csa_from, csa_piece, is_tsuitate, ignore_turn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_legality_extended::GameKind;

    #[test]
    fn test_make_a_move() {
        let mut game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        assert_eq!(game.last_move(), None);
        let result = game.make_move("+7776FU");

        assert!(result, "make_move should return true for +7776FU");
        assert_eq!(game.last_move().as_deref(), Some("+7776FU"));
        assert_eq!(
            game.sfen(),
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL w - 2"
        )
    }

    #[test]
    fn viewpoint_helpers_hide_opponent_state() {
        let mut game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        assert_eq!(
            game.sfen_for(Some('+'), None).unwrap(),
            "9/9/9/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
        );
        assert!(game.make_move("+7776FU"));
        assert_eq!(
            game.last_move_for(Some('+')).unwrap().as_deref(),
            Some("+7776FU")
        );
        assert_eq!(
            game.last_move_for(Some('-')).unwrap().as_deref(),
            Some("+0000ZZ")
        );
        assert!(game.sfen_for(Some('*'), None).is_err());
        assert!(game.sfen_for(Some('+'), Some(true)).unwrap().contains('?'));
    }

    #[test]
    fn test_make_moves() {
        let mut game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        game.make_move("+7776FU");
        let result = game.make_move("-3334FU");

        assert!(result, "make_move should return true");
        assert_eq!(
            game.sfen(),
            "lnsgkgsnl/1r5b1/pppppp1pp/6p2/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL b - 3"
        )
    }

    #[test]
    fn make_move_ignore_turn_restores_turn_after_failure() {
        let mut game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        assert!(!game.make_move_ignore_turn("+7775FU"));
        assert_eq!(
            game.sfen(),
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
        );
    }

    #[test]
    fn from_candidates_returns_empty_for_invalid_input() {
        let game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        assert!(
            game.from_candidates('*', "77", "FU", false, false)
                .is_empty()
        );
        assert!(
            game.from_candidates('+', "7", "FU", false, false)
                .is_empty()
        );
        assert!(
            game.from_candidates('+', "7a", "FU", false, false)
                .is_empty()
        );
        assert!(
            game.from_candidates('+', "00", "XX", false, false)
                .is_empty()
        );
        assert!(
            game.from_candidates('+', "99", "FU", false, false)
                .is_empty()
        );
    }

    #[test]
    fn new_sets_initial_last_info() {
        let game = WasmGame::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            Some(crate::game_api::INFO_CHECK),
        )
        .unwrap();

        assert_eq!(game.last_info(), Some(crate::game_api::INFO_CHECK));
    }

    #[test]
    fn new_returns_error_for_invalid_sfen() {
        assert!(
            WasmGame::new(
                "invalid",
                GameKind::Shogi.to_u8(),
                false,
                3,
                9,
                9,
                150,
                None
            )
            .is_err()
        );
        assert!(
            WasmGame::new(
                "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
                0,
                false,
                3,
                9,
                9,
                150,
                None,
            )
            .is_err()
        );
        assert!(
            WasmGame::new(
                "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
                GameKind::Shogi.to_u8(),
                false,
                3,
                9,
                9,
                150,
                Some(7),
            )
            .is_err()
        );
    }

    #[test]
    fn test_setup_from_rectangular_sfen() {
        let game = WasmGame::new(
            "sfen gks/1p1/1P1/SKG b - 1",
            GameKind::Dobutsu.to_u8(),
            true,
            1,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        assert_eq!(game.sfen(), "gks/1p1/1P1/SKG b - 1");
    }
}
