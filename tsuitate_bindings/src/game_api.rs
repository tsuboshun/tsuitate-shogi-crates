use std::fmt;

use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square, ToUsi};
use shogi_legality_extended::{GameKind, Setting, from_candidates, is_valid};
use shogi_usi_parser::FromUsi;
use tsuitate_game::{Game, Info, csa_to_move, csa_to_piece_kind, nth_ascii};

use crate::rl::{
    action_index_to_move as rl_action_index_to_move, infer_last_move_color,
    legal_action_indices_for_position, move_action_indices_to_square,
};
use crate::sfen_util::{SfenNormalizeError, denormalize_sfen_from_9x9, normalize_sfen_to_9x9};

pub(crate) const INFO_NONE: u8 = 0;
pub(crate) const INFO_FOUL: u8 = 1;
pub(crate) const INFO_FOUL_UNDER_CHECK: u8 = 2;
pub(crate) const INFO_CHECK: u8 = 3;
pub(crate) const INFO_CHECKMATE: u8 = 4;
pub(crate) const INFO_LOSS_BY_FOUL: u8 = 5;
pub(crate) const INFO_DRAW: u8 = 6;

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
                write!(f, "promotion_rank must be less than or equal to ranks")
            }
            Self::InvalidNormalizedSfen => write!(f, "invalid normalized sfen"),
            Self::InvalidLastInfo => write!(f, "invalid last_info"),
        }
    }
}

fn info_from_u8(value: u8) -> Option<Info> {
    match value {
        INFO_NONE => Some(Info::None),
        INFO_FOUL => Some(Info::Foul),
        INFO_FOUL_UNDER_CHECK => Some(Info::FoulUnderCheck),
        INFO_CHECK => Some(Info::Check),
        INFO_CHECKMATE => Some(Info::Checkmate),
        INFO_LOSS_BY_FOUL => Some(Info::LossByFoul),
        INFO_DRAW => Some(Info::Draw),
        _ => None,
    }
}

fn csa_coord_digit(value: &str, index: usize) -> Option<u8> {
    nth_ascii(value, index)
        .and_then(|ch| ch.to_digit(10))
        .map(|digit| digit as u8)
}

fn color_to_csa_sign(color: Color) -> char {
    match color {
        Color::Black => '+',
        Color::White => '-',
    }
}

fn piece_kind_to_csa(piece_kind: PieceKind) -> &'static str {
    match piece_kind {
        PieceKind::Pawn => "FU",
        PieceKind::Lance => "KY",
        PieceKind::Knight => "KE",
        PieceKind::Silver => "GI",
        PieceKind::Gold => "KI",
        PieceKind::Bishop => "KA",
        PieceKind::Rook => "HI",
        PieceKind::King => "OU",
        PieceKind::ProPawn => "TO",
        PieceKind::ProLance => "NY",
        PieceKind::ProKnight => "NK",
        PieceKind::ProSilver => "NG",
        PieceKind::ProBishop => "UM",
        PieceKind::ProRook => "RY",
    }
}

pub(crate) struct GameApi {
    inner: Game,
    setting: Setting,
}

impl GameApi {
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
            Some(value) => Some(info_from_u8(value).ok_or(GameApiError::InvalidLastInfo)?),
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

    pub(crate) fn last_move_csa(&self) -> Option<String> {
        let mv = self.last_move()?;
        let color = infer_last_move_color(self, &mv);
        let sign = color_to_csa_sign(color);

        match mv {
            Move::Drop { piece, to } => {
                let piece_csa = piece_kind_to_csa(piece.piece_kind());
                Some(format!("{}00{}{}{}", sign, to.file(), to.rank(), piece_csa))
            }
            Move::Normal { from, to, promote } => {
                let piece_kind = if promote {
                    self.position()
                        .piece_at(from)
                        .and_then(|piece| piece.piece_kind().promote())
                        .or_else(|| self.position().piece_at(to).map(|piece| piece.piece_kind()))
                } else {
                    self.position()
                        .piece_at(from)
                        .map(|piece| piece.piece_kind())
                        .or_else(|| self.position().piece_at(to).map(|piece| piece.piece_kind()))
                }?;
                let piece_csa = piece_kind_to_csa(piece_kind);
                Some(format!(
                    "{}{}{}{}{}{}",
                    sign,
                    from.file(),
                    from.rank(),
                    to.file(),
                    to.rank(),
                    piece_csa
                ))
            }
        }
    }

    pub(crate) fn last_capture_piece_kind(&self) -> Option<PieceKind> {
        self.inner.last_capture
    }

    pub(crate) fn king_position(&self, color: Color) -> Option<(u8, u8)> {
        self.position()
            .king_position(color)
            .map(|square| (square.file(), square.rank()))
    }

    pub(crate) fn legal_action_indices(&self, color: Color) -> Vec<usize> {
        let mut position = self.position().clone();
        position.side_to_move_set(color);
        legal_action_indices_for_position(&position, &self.setting)
    }

    pub(crate) fn action_index_to_move(&self, action_index: usize) -> Option<Move> {
        rl_action_index_to_move(self.position(), self.setting.is_tsuitate, action_index)
    }

    pub(crate) fn action_index_to_csa_move(&self, action_index: usize) -> Option<String> {
        let mv = self.action_index_to_move(action_index)?;
        self.move_to_csa(mv)
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

    pub(crate) fn move_to_csa(&self, mv: Move) -> Option<String> {
        match mv {
            Move::Drop { piece, to } => {
                let sign = color_to_csa_sign(piece.color());
                let piece_csa = piece_kind_to_csa(piece.piece_kind());
                Some(format!("{}00{}{}{}", sign, to.file(), to.rank(), piece_csa))
            }
            Move::Normal { from, to, promote } => {
                let piece = self.position().piece_at(from)?;
                let piece_kind = if promote {
                    piece.piece_kind().promote()?
                } else {
                    piece.piece_kind()
                };
                let sign = color_to_csa_sign(piece.color());
                let piece_csa = piece_kind_to_csa(piece_kind);
                Some(format!(
                    "{}{}{}{}{}{}",
                    sign,
                    from.file(),
                    from.rank(),
                    to.file(),
                    to.rank(),
                    piece_csa
                ))
            }
        }
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

    pub(crate) fn sfen(&self) -> String {
        denormalize_sfen_from_9x9(
            &self.inner.to_sfen_owned(),
            self.setting.files,
            self.setting.ranks,
        )
        .unwrap_or_else(|_| self.inner.to_sfen_owned())
    }

    pub(crate) fn fouls(&self) -> Vec<i8> {
        self.inner.fouls.iter().map(|foul| *foul as i8).collect()
    }

    pub(crate) fn set_fouls(&mut self, foul0: i8, foul1: i8) {
        self.inner.fouls[0] = foul0;
        self.inner.fouls[1] = foul1;
    }

    pub(crate) fn last_info(&self) -> Option<u8> {
        self.inner.last_info.map(|info| info as u8)
    }

    pub(crate) fn last_capture(&self) -> Option<String> {
        self.inner.last_capture.map(|pk| {
            let mut buf = String::new();
            pk.to_usi(&mut buf).unwrap();
            buf
        })
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
        let color = match csa_color {
            '+' => Color::Black,
            '-' => Color::White,
            _ => return Vec::new(),
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
    fn king_position_returns_file_and_rank_for_color() {
        let game = new_standard_game();

        assert_eq!(game.king_position(Color::Black), Some((5, 9)));
        assert_eq!(game.king_position(Color::White), Some((5, 1)));
    }

    #[test]
    fn legal_action_indices_use_requested_color() {
        let game = new_standard_game();
        let black_pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27;
        let white_pawn_push = ((3 - 1) * 9 + (4 - 1)) * 27 + 4;

        assert!(
            game.legal_action_indices(Color::Black)
                .contains(&black_pawn_push)
        );
        assert!(
            game.legal_action_indices(Color::White)
                .contains(&white_pawn_push)
        );
    }

    #[test]
    fn action_index_to_csa_move_returns_existing_move_format() {
        let game = new_standard_game();
        let black_pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27;

        assert_eq!(
            game.action_index_to_csa_move(black_pawn_push).as_deref(),
            Some("+7776FU")
        );
    }

    #[test]
    fn move_action_indices_to_returns_legal_normal_actions_for_current_state() {
        let mut game = new_standard_game();
        let black_pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27;
        let white_pawn_push = ((3 - 1) * 9 + (4 - 1)) * 27 + 4;

        assert_eq!(game.move_action_indices_to(7, 6), vec![black_pawn_push]);
        assert!(game.move_action_indices_to(3, 4).is_empty());
        assert!(game.make_move("+7776FU"));
        assert_eq!(game.move_action_indices_to(3, 4), vec![white_pawn_push]);
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
            Some(INFO_CHECK),
        )
        .unwrap();

        assert_eq!(game.last_info(), Some(INFO_CHECK));
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
