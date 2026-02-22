use tsify_next::Tsify;
use wasm_bindgen::prelude::*;
//use web_sys::console;

use shogi_core::{Color, PartialPosition, Piece, PieceKind, Square, ToUsi};
use shogi_legality_extended::{
    GameKind, Setting, from_candidates, is_valid, will_king_be_captured,
};
use shogi_usi_parser::FromUsi;
use tsuitate_game::{Game, csa_to_move, csa_to_piece_kind, nth_ascii};

use crate::sfen_util::{denormalize_sfen_from_9x9, normalize_sfen_to_9x9};

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
    inner: Game,
    setting: Setting,
}

#[wasm_bindgen]
impl WasmGame {
    #[wasm_bindgen(constructor)]
    pub fn new(
        sfen: &str,
        game_kind: u8,
        is_tsuitate: bool,
        promotion_ranks: u8,
        foul0: i8,
        foul1: i8,
        draw_move_count: u16,
    ) -> WasmGame {
        let game_kind = match GameKind::from_u8(game_kind) {
            Some(value) => value,
            None => panic!("Unknown game kind"),
        };
        let (sfen, files, ranks) = normalize_sfen_to_9x9(sfen);
        let setting = Setting::new(files, ranks, promotion_ranks, game_kind, is_tsuitate);

        //console::log_1(&JsValue::from(sfen));
        let initial = PartialPosition::from_usi(&sfen).unwrap();

        WasmGame {
            inner: Game::new(initial, [foul0, foul1], draw_move_count),
            setting,
        }
    }

    pub(crate) fn position(&self) -> &PartialPosition {
        &self.inner.inner
    }

    pub(crate) fn position_mut(&mut self) -> &mut PartialPosition {
        &mut self.inner.inner
    }

    #[wasm_bindgen]
    pub fn make_move(&mut self, csa_move: &str) -> bool {
        let mv = csa_to_move(csa_move, self.position());
        match self.inner.make_move(mv, &self.setting) {
            Some(_) => true,
            None => false,
        }
    }

    #[wasm_bindgen]
    pub fn make_move_ignore_turn(&mut self, csa_move: &str) -> bool {
        let mv = csa_to_move(csa_move, self.position());
        match self.inner.make_move(mv, &self.setting) {
            Some(_) => true,
            None => {
                let side = self.position().side_to_move().flip();
                self.position_mut().side_to_move_set(side);
                match self.inner.make_move(mv, &self.setting) {
                    Some(_) => true,
                    None => false,
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn sfen(&self) -> String {
        denormalize_sfen_from_9x9(
            &self.inner.to_sfen_owned(),
            self.setting.files,
            self.setting.ranks,
        )
    }

    #[wasm_bindgen(getter)]
    pub fn fouls(&self) -> Vec<i8> {
        self.inner.fouls.iter().map(|foul| *foul as i8).collect()
    }

    #[wasm_bindgen]
    pub fn set_fouls(&mut self, foul0: i8, foul1: i8) {
        self.inner.fouls[0] = foul0;
        self.inner.fouls[1] = foul1;
    }

    #[wasm_bindgen(getter)]
    pub fn last_info(&self) -> Option<u8> {
        self.inner.infos.last().map(|info| *info as u8)
    }

    #[wasm_bindgen(getter)]
    pub fn last_capture(&self) -> Option<String> {
        self.inner.captures.last().cloned().map(|opt| match opt {
            Some(pk) => {
                let mut buf = String::new();
                pk.to_usi(&mut buf).unwrap();
                buf
            }
            None => String::new(),
        })
    }

    #[wasm_bindgen]
    pub fn is_valid(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        let mv = csa_to_move(csa_move, self.position());
        is_valid(
            self.position(),
            mv,
            &Setting {
                is_tsuitate,
                ..self.setting
            },
        )
    }

    #[wasm_bindgen]
    pub fn is_valid_ignore_turn(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        let mv = csa_to_move(csa_move, self.position());
        if is_valid(
            self.position(),
            mv,
            &Setting {
                is_tsuitate,
                ..self.setting
            },
        ) {
            true
        } else {
            let mut temp_position = self.position().clone();
            temp_position.side_to_move_set(temp_position.side_to_move().flip());
            is_valid(
                &temp_position,
                mv,
                &Setting {
                    is_tsuitate,
                    ..self.setting
                },
            )
        }
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
        let file_from = nth_ascii(csa_from, 0).unwrap().to_digit(10).unwrap() as u8;
        let rank_from = nth_ascii(csa_from, 1).unwrap().to_digit(10).unwrap() as u8;
        let color = match csa_color {
            '+' => Color::Black,
            '-' => Color::White,
            _ => panic!("invalid color"),
        };

        // Special handling for knight moves
        let setting = match csa_piece {
            "KE" => Setting::new(
                self.setting.files,
                self.setting.ranks,
                9,
                self.setting.game_kind,
                self.setting.is_tsuitate,
            ),
            _ => self.setting.clone(),
        };

        let mut bitboard = if file_from == 0 && rank_from == 0 {
            !self.position().player_bitboard(color)
        } else {
            let square_from = Square::new(file_from, rank_from).unwrap();
            let piece_kind = csa_to_piece_kind(csa_piece);
            let piece = Piece::new(piece_kind, color);
            from_candidates(self.position(), piece, square_from, &setting)
        };
        bitboard &= setting.board_mask;

        let csa_piece = if csa_piece == "KE" && csa_from != "00" {
            "NK"
        } else {
            csa_piece
        };

        let opponent_king_square = self.position().king_position(color.flip());
        let mut ret = Vec::new();
        while let Some(to) = bitboard.pop() {
            let csa_to = format!("{}{}", to.file(), to.rank());
            let csa_move = format!("{}{}{}{}", csa_color, csa_from, csa_to, csa_piece);
            let mv = csa_to_move(&csa_move, self.position());

            // Capturing king: avoid dropping candidates because `make_move` rejects king capture.
            if let (Some(opponent_king), Some(from)) = (opponent_king_square, mv.from()) {
                if mv.to() == opponent_king {
                    if let Some(piece) = self.position().piece_at(from) {
                        if piece.piece_kind() != PieceKind::King {
                            let mut pos_without_from = self.position().clone();
                            pos_without_from.piece_set(from, None);
                            let is_my_king_in_check = will_king_be_captured(
                                &pos_without_from,
                                color.flip(),
                                setting.game_kind,
                            )
                            .unwrap_or(false);
                            if !is_my_king_in_check
                                && (ignore_turn
                                    && self.is_valid_ignore_turn(&csa_move, is_tsuitate))
                                || (!ignore_turn && self.is_valid(&csa_move, is_tsuitate))
                            {
                                ret.push(csa_to);
                                continue;
                            }
                        }
                    }
                }
            }

            if ignore_turn {
                let mut game = self.inner.clone();
                if game.make_move(mv, &setting).is_some() {
                    ret.push(csa_to);
                } else {
                    game.inner
                        .side_to_move_set(game.inner.side_to_move().flip());
                    if game.make_move(mv, &setting).is_some() {
                        ret.push(csa_to);
                    }
                }
            } else {
                let mut game = self.inner.clone();
                if game.make_move(mv, &setting).is_some() {
                    ret.push(csa_to);
                }
            }
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        );
        let result = game.make_move("+7776FU");

        assert!(result, "make_move should return true for +7776FU");
        assert_eq!(
            game.sfen(),
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL w - 2"
        )
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
        );
        game.make_move("+7776FU");
        let result = game.make_move("-3334FU");

        assert!(result, "make_move should return true");
        assert_eq!(
            game.sfen(),
            "lnsgkgsnl/1r5b1/pppppp1pp/6p2/9/2P6/PP1PPPPPP/1B5R1/LNSGKGSNL b - 3"
        )
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
        );
        assert_eq!(game.sfen(), "gks/1p1/1P1/SKG b - 1");
    }
}
