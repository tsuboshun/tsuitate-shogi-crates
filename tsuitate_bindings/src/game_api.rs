use std::fmt;

use shogi_core::{Bitboard, Color, Hand, Move, PartialPosition, Piece, PieceKind, Square, ToUsi};
use shogi_legality_extended::{
    GameKind, Setting, from_candidates, from_candidates_without_assertion, is_valid,
};
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

fn piece_to_sfen(piece: Piece) -> String {
    let mut buf = String::new();
    piece.to_usi(&mut buf).unwrap();
    buf
}

#[inline]
fn is_sliding_piece(piece_kind: PieceKind) -> bool {
    matches!(
        piece_kind,
        PieceKind::Lance
            | PieceKind::Bishop
            | PieceKind::Rook
            | PieceKind::ProBishop
            | PieceKind::ProRook
    )
}

/// Returns the squares whose file and rank are both within `max_distance` of
/// `from`. `max_distance` must be less than 8.
#[inline]
fn sliding_distance_mask(from: Square, max_distance: u8) -> Bitboard {
    debug_assert!(max_distance < 8);

    let min_file = from.file().saturating_sub(max_distance).max(1);
    let max_file = from.file().saturating_add(max_distance).min(9);
    let min_rank = from.rank().saturating_sub(max_distance).max(1);
    let max_rank = from.rank().saturating_add(max_distance).min(9);
    let rank_pattern = ((1u16 << (max_rank - min_rank + 1)) - 1) << (min_rank - 1);
    let mut mask = Bitboard::empty();
    for file in min_file..=max_file {
        // Safety: the bounds above keep `file` in 1..=9 and `rank_pattern`
        // within the low nine bits.
        mask |= unsafe { Bitboard::from_file_unchecked(file, rank_pattern) };
    }
    mask
}

#[derive(Clone)]
pub(crate) struct GameApi {
    inner: Game,
    setting: Setting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MoveAnalysis {
    pub(crate) csa_move: String,
    pub(crate) valid: bool,
    pub(crate) last_info: Option<u8>,
    pub(crate) last_capture: Option<String>,
    pub(crate) sfen: String,
    pub(crate) fouls: (i8, i8),
    pub(crate) attack_counts: Option<Vec<u8>>,
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

    pub(crate) fn sfen(&self, viewpoint: Option<Color>, is_dark_shogi: bool) -> String {
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

    fn fouls_tuple(&self) -> (i8, i8) {
        (self.inner.fouls[0], self.inner.fouls[1])
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

    /// Returns attack counts in SFEN board order: rank 1 to rank N, and within
    /// each rank file N to file 1.
    ///
    /// When `treat_friendly_target_as_empty` is `true`, a square occupied by a
    /// friendly piece is treated as empty only when deciding whether that
    /// square itself is attacked. The piece still blocks attacks beyond it, so
    /// a friendly piece on a rook, bishop, or lance ray remains an obstacle.
    /// When it is `false`, every square occupied by a friendly piece has an
    /// attack count of zero; friendly pieces still block attacks beyond them.
    pub(crate) fn attack_counts(
        &self,
        color: Color,
        treat_friendly_target_as_empty: bool,
        max_sliding_distance: Option<u8>,
    ) -> Vec<u8> {
        self.attack_counts_with_tsuitate_setting(
            color,
            treat_friendly_target_as_empty,
            max_sliding_distance,
            self.setting.is_tsuitate,
        )
    }

    fn attack_counts_with_tsuitate_setting(
        &self,
        color: Color,
        treat_friendly_target_as_empty: bool,
        max_sliding_distance: Option<u8>,
        is_tsuitate: bool,
    ) -> Vec<u8> {
        let mut target_position = self.position().clone();
        if treat_friendly_target_as_empty {
            for square in Square::all() {
                if target_position
                    .piece_at(square)
                    .is_some_and(|piece| piece.color() == color)
                {
                    target_position.piece_set(square, None);
                }
            }
        }

        let occupied = if is_tsuitate {
            self.position().player_bitboard(color)
        } else {
            !self.position().vacant_bitboard()
        };
        let mut counts = vec![0u8; self.setting.files as usize * self.setting.ranks as usize];
        let mut pieces = self.position().player_bitboard(color) & self.setting.board_mask;
        while let Some(from) = pieces.pop() {
            let Some(piece) = self.position().piece_at(from) else {
                continue;
            };
            let mut attacks = from_candidates_without_assertion(
                occupied,
                &target_position,
                piece,
                from.file(),
                from.rank(),
                self.setting.game_kind,
            ) & self.setting.board_mask;

            if let Some(max_distance) = max_sliding_distance
                && max_distance < 8
                && is_sliding_piece(piece.piece_kind())
            {
                attacks &= sliding_distance_mask(from, max_distance);
            }

            while let Some(square) = attacks.pop() {
                let index = (square.rank() as usize - 1) * self.setting.files as usize
                    + (self.setting.files - square.file()) as usize;
                counts[index] += 1;
            }
        }
        counts
    }

    pub(crate) fn analyze_moves(
        &self,
        moves: &[String],
        attack_color: Color,
        include_attack_counts: bool,
        treat_friendly_target_as_empty: bool,
        max_sliding_distance: Option<u8>,
    ) -> Vec<MoveAnalysis> {
        moves
            .iter()
            .map(|csa_move| {
                let mut game = self.clone();
                let valid = game.make_move(csa_move);
                let attack_counts = include_attack_counts.then(|| {
                    game.attack_counts_with_tsuitate_setting(
                        attack_color,
                        treat_friendly_target_as_empty,
                        max_sliding_distance,
                        false,
                    )
                });
                MoveAnalysis {
                    csa_move: csa_move.clone(),
                    valid,
                    last_info: game.last_info(),
                    last_capture: game.last_capture(),
                    sfen: game.sfen(None, false),
                    fouls: game.fouls_tuple(),
                    attack_counts,
                }
            })
            .collect()
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
        let white_pawn_push = ((3 - 1) * 9 + (4 - 1)) * 27;

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
    fn move_action_indices_to_returns_legal_normal_actions_for_current_state() {
        let mut game = new_standard_game();
        let black_pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27;
        let white_pawn_push = ((3 - 1) * 9 + (4 - 1)) * 27;

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

    #[test]
    fn attack_counts_can_include_friendly_occupied_targets() {
        let game = GameApi::new(
            "sfen 9/9/9/9/9/4P4/9/4R4/9 b - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        let index = |file: usize, rank: usize| (rank - 1) * 9 + (9 - file);

        let counts = game.attack_counts(Color::Black, true, None);
        assert_eq!(counts.len(), 81);
        assert_eq!(counts[index(5, 7)], 1);
        assert_eq!(counts[index(5, 6)], 1);
        assert_eq!(counts[index(5, 5)], 1); // the pawn attacks this square

        let counts = game.attack_counts(Color::Black, false, None);
        assert_eq!(counts[index(5, 6)], 0);
    }

    #[test]
    fn sliding_distance_mask_matches_chebyshev_distance() {
        for from in Square::all() {
            for max_distance in 0..8 {
                let mask = sliding_distance_mask(from, max_distance);
                for target in Square::all() {
                    let expected = from
                        .file()
                        .abs_diff(target.file())
                        .max(from.rank().abs_diff(target.rank()))
                        <= max_distance;
                    assert_eq!(mask.contains(target), expected, "{from:?}, {target:?}");
                }
            }
        }
    }

    #[test]
    fn analyze_moves_evaluates_independent_clones() {
        let game = new_standard_game();
        let original_sfen = game.sfen(None, false);
        let moves = vec!["+7776FU".to_owned(), "+7775FU".to_owned()];

        let results = game.analyze_moves(&moves, Color::Black, true, true, None);

        assert_eq!(results.len(), 2);
        assert!(results[0].valid);
        assert_eq!(results[0].last_info, Some(INFO_NONE));
        assert_eq!(results[0].fouls, (9, 9));
        assert_eq!(results[0].attack_counts.as_ref().unwrap().len(), 81);
        assert!(!results[1].valid);
        assert_eq!(results[1].sfen, original_sfen);
        assert_eq!(game.sfen(None, false), original_sfen);
        assert_eq!(game.last_move(), None);
    }

    #[test]
    fn analyze_moves_calculates_attacks_as_a_normal_board() {
        let game = GameApi::new(
            "sfen 9/9/9/4p4/9/9/P8/4R4/9 b - 1",
            GameKind::Shogi.to_u8(),
            true,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        let index = |file: usize, rank: usize| (rank - 1) * 9 + (9 - file);

        let tsuitate_counts = game.attack_counts(Color::Black, true, None);
        assert_eq!(tsuitate_counts[index(5, 3)], 1);

        let results = game.analyze_moves(&["+9796FU".to_owned()], Color::Black, true, true, None);
        let counts = results[0].attack_counts.as_ref().unwrap();

        assert!(results[0].valid);
        assert_eq!(counts[index(5, 4)], 1);
        assert_eq!(counts[index(5, 3)], 0);
    }
}
