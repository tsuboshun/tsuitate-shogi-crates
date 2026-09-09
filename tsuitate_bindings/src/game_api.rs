#![allow(dead_code)]
use std::fmt;

use shogi_core::{Bitboard, Color, Hand, Move, PartialPosition, Piece, PieceKind, Square};
use shogi_legality_extended::{GameKind, Setting, from_candidates, is_valid};
use shogi_usi_parser::FromUsi;
use tsuitate_game::{
    Game, Info, color_to_csa_sign, csa_coord_digit, csa_sign_to_color, csa_to_move,
    csa_to_piece_kind, move_to_csa, piece_kind_to_sfen, piece_to_sfen,
};

use crate::rl::{
    action_index_to_move as rl_action_index_to_move, infer_last_move_color,
    legal_action_indices_for_position, move_action_indices_to_square,
};
use crate::sfen_util::{SfenNormalizeError, denormalize_sfen_from_9x9, normalize_sfen_to_9x9};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GameApiError {
    UnknownGameKind,
    InvalidSfen(SfenNormalizeError),
    PromotionRanksTooLarge,
    InvalidNormalizedSfen,
    InvalidLastInfo,
}

impl fmt::Display for GameApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownGameKind => write!(f, "unknown game kind"),
            Self::InvalidSfen(err) => write!(f, "invalid sfen: {err:?}"),
            Self::PromotionRanksTooLarge => {
                return write!(f, "promotion_rank must be less than or equal to ranks");
            }
            Self::InvalidNormalizedSfen => write!(f, "invalid normalized sfen"),
            Self::InvalidLastInfo => write!(f, "invalid last_info"),
        }
    }
}

#[derive(Clone)]
pub struct GameApi {
    inner: Game,
    setting: Setting,
}

impl GameApi {
    pub fn from_game(inner: Game, setting: Setting) -> Self {
        Self { inner, setting }
    }

    pub(crate) fn new(
        sfen: &str,
        game_kind: u8,
        is_tsuitate: bool,
        promotion_rank: u8,
        foul0: i8,
        foul1: i8,
        draw_move_count: u16,
        last_info: Option<u8>,
    ) -> Result<Self, GameApiError> {
        let game_kind = GameKind::from_u8(game_kind).ok_or(GameApiError::UnknownGameKind)?;
        let (sfen, files, ranks) =
            normalize_sfen_to_9x9(sfen).map_err(GameApiError::InvalidSfen)?;
        if promotion_rank > ranks {
            return Err(GameApiError::PromotionRanksTooLarge);
        }
        let setting = Setting::new(files, ranks, promotion_rank, game_kind, is_tsuitate);

        let initial =
            PartialPosition::from_usi(&sfen).map_err(|_| GameApiError::InvalidNormalizedSfen)?;
        let last_info = match last_info {
            Some(value) => Some(Info::try_from(value).map_err(|_| GameApiError::InvalidLastInfo)?),
            None => None,
        };

        Ok(Self {
            inner: Game::new(initial, [foul0, foul1], draw_move_count, last_info),
            setting,
        })
    }

    pub(crate) fn position(&self) -> &PartialPosition {
        &self.inner.inner
    }

    pub(crate) fn position_mut(&mut self) -> &mut PartialPosition {
        &mut self.inner.inner
    }

    pub(crate) fn setting(&self) -> &Setting {
        &self.setting
    }

    pub(crate) fn last_move(&self) -> Option<Move> {
        self.inner.last_move
    }

    pub(crate) fn last_move_csa(&self, viewpoint: Option<Color>) -> Option<String> {
        let mv = self.last_move()?;
        let color = infer_last_move_color(self, &mv);
        let sign = color_to_csa_sign(color);

        if viewpoint.is_some_and(|viewpoint| viewpoint != color) {
            return Some(match mv {
                Move::Normal { to, .. } if self.inner.last_capture.is_some() => {
                    format!("{}00{}{}ZZ", sign, to.file(), to.rank())
                }
                _ => format!("{}0000ZZ", sign),
            });
        }

        move_to_csa(mv, self.position())
    }

    pub(crate) fn last_capture_piece_kind(&self) -> Option<PieceKind> {
        self.inner.last_capture
    }

    pub(crate) fn king_position(&self, color: Color) -> Option<(u8, u8)> {
        self.position()
            .king_position(color)
            .map(|square| (square.file(), square.rank()))
    }

    pub fn legal_action_indices(
        &self,
        color: Color,
        history: Option<&crate::ActionHistory<'_>>,
    ) -> Vec<usize> {
        if self.position().side_to_move() == color {
            return legal_action_indices_for_position(self.position(), &self.setting, history);
        }
        let mut position = self.position().clone();
        position.side_to_move_set(color);
        legal_action_indices_for_position(&position, &self.setting, history)
    }

    /// Decode using the game's current side to move.
    /// Pass indices generated for that side; decoding alone does not validate a move.
    pub fn action_index_to_move(&self, action_index: usize) -> Option<Move> {
        rl_action_index_to_move(self.position(), self.setting.is_tsuitate, action_index)
    }

    pub(crate) fn action_index_to_csa_move(&self, action_index: usize) -> Option<String> {
        let mv = self.action_index_to_move(action_index)?;
        move_to_csa(mv, self.position())
    }

    pub(crate) fn move_action_indices_to(&self, file: u8, rank: u8) -> Vec<usize> {
        move_action_indices_to_square(file, rank)
            .into_iter()
            .filter(|action_index| {
                self.action_index_to_move(*action_index).is_some_and(|mv| {
                    matches!(mv, Move::Normal { .. })
                        && is_valid(self.position(), mv, &self.setting)
                })
            })
            .collect()
    }

    pub(crate) fn make_move_raw(&mut self, mv: Move) -> bool {
        self.inner.make_move(mv, &self.setting).is_some()
    }

    pub(crate) fn make_move(&mut self, csa_move: &str) -> bool {
        let mv = match csa_to_move(csa_move, self.position()) {
            Ok(mv) => mv,
            Err(_) => return false,
        };
        self.make_move_raw(mv)
    }

    pub(crate) fn make_move_ignore_turn(&mut self, csa_move: &str) -> bool {
        let mv = match csa_to_move(csa_move, self.position()) {
            Ok(mv) => mv,
            Err(_) => return false,
        };
        match self.inner.make_move(mv, &self.setting) {
            Some(_) => true,
            None => {
                let original_side = self.position().side_to_move();
                self.position_mut().side_to_move_set(original_side.flip());
                let success = self.make_move_raw(mv);
                if !success {
                    self.position_mut().side_to_move_set(original_side);
                }
                success
            }
        }
    }

    fn dark_shogi_visible_squares(&self, viewpoint: Color) -> Bitboard {
        let mut position = self.position().clone();
        position.side_to_move_set(viewpoint);
        let visibility_setting = Setting {
            is_tsuitate: false,
            ..self.setting.clone()
        };
        let mut visible = position.player_bitboard(viewpoint) & self.setting.board_mask;
        let mut pieces = visible;
        while let Some(square) = pieces.pop() {
            let Some(piece) = position.piece_at(square) else {
                continue;
            };
            visible |= from_candidates(&position, piece, square, &visibility_setting);
        }
        visible & self.setting.board_mask
    }

    fn visible_board_sfen(&self, visible: Bitboard) -> String {
        let mut board = String::new();
        for rank in 1..=9 {
            let mut vacant = 0u8;
            for file in (1..=9).rev() {
                let square = Square::new(file, rank).unwrap();
                if !visible.contains(square) {
                    if vacant > 0 {
                        board.push_str(&vacant.to_string());
                        vacant = 0;
                    }
                    board.push('?');
                    continue;
                }

                match self.position().piece_at(square) {
                    Some(piece) => {
                        if vacant > 0 {
                            board.push_str(&vacant.to_string());
                            vacant = 0;
                        }
                        board.push_str(&piece_to_sfen(piece));
                    }
                    _ => vacant += 1,
                }
            }
            if vacant > 0 {
                board.push_str(&vacant.to_string());
            }
            if rank < 9 {
                board.push('/');
            }
        }
        board
    }

    /// SFEN cropped to the configured board size. A viewpoint hides the opponent's
    /// hand and board pieces, or reveals attacked squares in dark shogi (`?` elsewhere).
    /// `None` returns the full position, regardless of `is_dark_shogi`.
    pub fn sfen(&self, viewpoint: Option<Color>, is_dark_shogi: bool) -> String {
        let sfen = if let Some(viewpoint) = viewpoint {
            let mut position = self.position().clone();
            if is_dark_shogi {
                let visible = self.dark_shogi_visible_squares(viewpoint);
                let board = self.visible_board_sfen(visible);
                *position.hand_of_a_player_mut(viewpoint.flip()) = Hand::new();
                let suffix_source = position.to_sfen_owned();
                let suffix = suffix_source
                    .split_once(' ')
                    .map(|(_, suffix)| suffix)
                    .unwrap_or("");
                format!("{board} {suffix}")
            } else {
                for square in Square::all() {
                    if position
                        .piece_at(square)
                        .is_some_and(|piece| piece.color() != viewpoint)
                    {
                        position.piece_set(square, None);
                    }
                }
                *position.hand_of_a_player_mut(viewpoint.flip()) = Hand::new();
                position.to_sfen_owned()
            }
        } else {
            self.inner.to_sfen_owned()
        };

        denormalize_sfen_from_9x9(&sfen, self.setting.files, self.setting.ranks).unwrap_or(sfen)
    }

    pub(crate) fn fouls(&self) -> Vec<i8> {
        self.inner.fouls.iter().map(|foul| *foul as i8).collect()
    }

    pub(crate) fn set_fouls(&mut self, foul0: i8, foul1: i8) {
        self.inner.fouls[0] = foul0;
        self.inner.fouls[1] = foul1;
    }

    pub(crate) fn last_info(&self) -> Option<Info> {
        self.inner.last_info
    }

    pub(crate) fn last_capture(&self) -> Option<String> {
        self.inner.last_capture.map(piece_kind_to_sfen)
    }

    pub(crate) fn is_valid(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        let mv = match csa_to_move(csa_move, self.position()) {
            Ok(mv) => mv,
            Err(_) => return false,
        };
        is_valid(
            self.position(),
            mv,
            &Setting {
                is_tsuitate,
                ..self.setting
            },
        )
    }

    pub(crate) fn is_valid_ignore_turn(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        let mv = match csa_to_move(csa_move, self.position()) {
            Ok(mv) => mv,
            Err(_) => return false,
        };
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

    pub(crate) fn from_candidates(
        &self,
        csa_color: char,
        csa_from: &str,
        csa_piece: &str,
        is_tsuitate: bool,
        ignore_turn: bool,
    ) -> Vec<String> {
        let file_from = match csa_coord_digit(csa_from, 0) {
            Some(value) => value,
            None => return Vec::new(),
        };
        let rank_from = match csa_coord_digit(csa_from, 1) {
            Some(value) => value,
            None => return Vec::new(),
        };
        let Some(color) = csa_sign_to_color(csa_color) else {
            return Vec::new();
        };

        let setting = match csa_piece {
            "KE" => Setting {
                promotion_rank: self.setting.ranks,
                is_tsuitate,
                ..self.setting.clone()
            },
            "KY" => Setting {
                promotion_rank: self.setting.ranks,
                is_tsuitate,
                ..self.setting.clone()
            },
            _ => Setting {
                is_tsuitate,
                ..self.setting.clone()
            },
        };

        let mut bitboard = if file_from == 0 && rank_from == 0 {
            !self.position().player_bitboard(color)
        } else {
            let square_from = match Square::new(file_from, rank_from) {
                Some(square) => square,
                None => return Vec::new(),
            };
            let piece_kind = match csa_to_piece_kind(csa_piece) {
                Ok(piece_kind) => piece_kind,
                Err(_) => return Vec::new(),
            };
            let piece = Piece::new(piece_kind, color);
            if self.position().piece_at(square_from) != Some(piece) {
                return Vec::new();
            }
            from_candidates(self.position(), piece, square_from, &setting)
        };
        bitboard &= setting.board_mask;

        let csa_piece = if csa_piece == "KE" && csa_from != "00" {
            "NK"
        } else if csa_piece == "KY" && csa_from != "00" {
            "NY"
        } else {
            csa_piece
        };

        let mut ret = Vec::new();
        while let Some(to) = bitboard.pop() {
            let csa_to = format!("{}{}", to.file(), to.rank());
            let csa_move = format!("{}{}{}{}", csa_color, csa_from, csa_to, csa_piece);
            let mv = match csa_to_move(&csa_move, self.position()) {
                Ok(mv) => mv,
                Err(_) => continue,
            };

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

    fn new_standard_game() -> GameApi {
        GameApi::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap()
    }

    #[test]
    fn sfen_hides_opponent_board_and_hand_for_viewpoint() {
        let game = GameApi::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b Pp 1",
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
            game.sfen(Some(Color::Black), false),
            "9/9/9/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b P 1"
        );
        assert_eq!(
            game.sfen(Some(Color::White), false),
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/9/9/9 b p 1"
        );
    }

    #[test]
    fn dark_shogi_sfen_marks_unseen_squares_and_reveals_attacks() {
        let game = GameApi::new(
            "sfen 9/9/9/9/9/4p4/9/4R4/4K4 b Pp 1",
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
            game.sfen(Some(Color::Black), true),
            "?????????/?????????/?????????/?????????/?????????/????p????/????1????/4R4/???1K1??? b P 1"
        );
    }

    #[test]
    fn new_treats_dark_shogi_unknown_squares_as_empty() {
        let game = GameApi::new(
            "sfen ?????????/?????????/?????????/?????????/?????????/????p????/????1????/4R4/???1K1??? b P 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        assert_eq!(game.sfen(None, false), "9/9/9/9/9/4p4/9/4R4/4K4 b P 1");
    }

    #[test]
    fn last_move_csa_hides_opponent_move_without_capture() {
        let mut game = new_standard_game();
        assert!(game.make_move("+7776FU"));

        assert_eq!(
            game.last_move_csa(Some(Color::Black)).as_deref(),
            Some("+7776FU")
        );
        assert_eq!(
            game.last_move_csa(Some(Color::White)).as_deref(),
            Some("+0000ZZ")
        );
    }

    #[test]
    fn last_move_csa_reveals_only_destination_for_opponent_capture() {
        let mut game = GameApi::new(
            "sfen 9/9/9/9/9/9/9/9/9 w - 2",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        game.inner.last_move = Some(Move::Normal {
            from: Square::new(5, 9).unwrap(),
            to: Square::new(5, 7).unwrap(),
            promote: false,
        });
        game.inner.last_capture = Some(PieceKind::Pawn);

        assert_eq!(
            game.last_move_csa(Some(Color::White)).as_deref(),
            Some("+0057ZZ")
        );
    }

    #[test]
    fn new_sets_initial_last_info() {
        let game = GameApi::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            Some(Info::Check as u8),
        )
        .unwrap();

        assert_eq!(game.last_info(), Some(Info::Check));
    }

    #[test]
    fn new_rejects_invalid_last_info() {
        let result = GameApi::new(
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            Some(7),
        );

        assert!(matches!(result, Err(GameApiError::InvalidLastInfo)));
    }
}
