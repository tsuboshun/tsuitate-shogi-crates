//! A set of functions for generating features for reinforcement learning.
//!
//! The features are designed based on the book 山岡忠夫, 加納邦彦『強い将棋ソフトの創りかた』マイナビ出版, 2021.

#![allow(dead_code)]
use crate::game_api::GameApi;
use shogi_core::{Bitboard, Color, Move, PartialPosition, Piece, PieceKind, Square};
use shogi_legality_extended::{
    GameKind, Setting, drop_candidates, is_valid, normal_move_candidates,
};
use tsuitate_game::Info;

pub(crate) const ACTION_COUNT: usize = 9 * 9 * 27;
const ACTIONS_PER_SQUARE: usize = 27;
// 27 possible actions per destination square: 10 move directions x 2 (promote or not) + 7 drop piece kinds
const MOVE_ACTIONS: usize = 10;
const PROMOTE_ACTION_OFFSET: usize = 10;
const DROP_ACTION_OFFSET: usize = 20;

const OBSERVATION_CHANNELS_PER_SIDE: usize = 74;
const OBSERVATION_SIDE_COUNT: usize = 2;
pub(crate) const OBSERVATION_CHANNEL_COUNT: usize =
    OBSERVATION_CHANNELS_PER_SIDE * OBSERVATION_SIDE_COUNT;
pub(crate) const OBSERVATION_BYTES: usize = OBSERVATION_CHANNEL_COUNT * 16; // bitboards are 16 bytes each

const BOARD_OBSERVATION_PIECE_KINDS: [PieceKind; 14] = [
    PieceKind::Pawn,
    PieceKind::Lance,
    PieceKind::Knight,
    PieceKind::Silver,
    PieceKind::Gold,
    PieceKind::Bishop,
    PieceKind::Rook,
    PieceKind::King,
    PieceKind::ProPawn,
    PieceKind::ProLance,
    PieceKind::ProKnight,
    PieceKind::ProSilver,
    PieceKind::ProBishop,
    PieceKind::ProRook,
];

const HAND_OBSERVATION_SPECS: [(PieceKind, usize); 7] = [
    (PieceKind::Pawn, 18),
    (PieceKind::Lance, 4),
    (PieceKind::Knight, 4),
    (PieceKind::Silver, 4),
    (PieceKind::Gold, 4),
    (PieceKind::Bishop, 2),
    (PieceKind::Rook, 2),
];
const HAND_OBSERVATION_BINARY_CHANNELS: usize = 18 + 4 + 4 + 4 + 4 + 2 + 2; // 38

const OBSERVATION_SIDES: [Color; 2] = [Color::Black, Color::White];

// Observation are represented as a set of bitboards, one for each channel.
// First half of the channels are for Black's perspective, second half for White's perspective.
// The channels of each side are organized as follows:
const OBS_BOARD_CHANNEL_OFFSET: usize = 0;
const OBS_HAND_CHANNEL_OFFSET: usize =
    OBS_BOARD_CHANNEL_OFFSET + BOARD_OBSERVATION_PIECE_KINDS.len();
// 14 (The board is encoded using bitboards for each piece type)
const OBS_LAST_MOVE_FROM_MOVE_CHANNEL: usize =
    OBS_HAND_CHANNEL_OFFSET + HAND_OBSERVATION_BINARY_CHANNELS;
// 52 (Hand pieces are encoded using bitboards consisting entirely of 0s or 1s for each piece type x count combination)
const OBS_LAST_MOVE_FROM_DROP_OFFSET: usize = OBS_LAST_MOVE_FROM_MOVE_CHANNEL + 1; // 53
const OBS_LAST_MOVE_TO_CHANNEL: usize = OBS_LAST_MOVE_FROM_DROP_OFFSET + DROP_PIECE_KINDS.len(); // 60
const OBS_LAST_CAPTURE_KIND_OFFSET: usize = OBS_LAST_MOVE_TO_CHANNEL + 1; // 61
const OBS_LAST_CAPTURE_POSITION_CHANNEL: usize =
    OBS_LAST_CAPTURE_KIND_OFFSET + DROP_PIECE_KINDS.len(); // 68
const OBS_LAST_INFO_OFFSET: usize = OBS_LAST_CAPTURE_POSITION_CHANNEL + 1; // 69
const OBS_COLOR_CHANNEL: usize = OBS_LAST_INFO_OFFSET + 4; // 73

// (file_delta, rank_delta) from the moving side's viewpoint for Black:
// [forward, forward-right, right, ..., left, forward-left,
// knight forward-right, knight forward-left].
// White uses the same action kinds rotated 180 degrees.
const MOVE_DELTAS: [(i8, i8); MOVE_ACTIONS] = [
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (-1, -2),
    (1, -2),
];

const DROP_PIECE_KINDS: [PieceKind; 7] = [
    PieceKind::Pawn,
    PieceKind::Lance,
    PieceKind::Knight,
    PieceKind::Silver,
    PieceKind::Gold,
    PieceKind::Bishop,
    PieceKind::Rook,
];

#[inline(always)]
fn action_to_square_and_kind(action_index: usize) -> Option<(Square, usize)> {
    if action_index >= ACTION_COUNT {
        return None;
    }
    let to_array_index = action_index / ACTIONS_PER_SQUARE;
    let action_kind = action_index % ACTIONS_PER_SQUARE;
    // Safety: to_array_index is in 0..81.
    let to = unsafe { Square::from_u8_unchecked((to_array_index + 1) as u8) };
    Some((to, action_kind))
}

#[inline(always)]
fn find_from_on_ray(
    position: &PartialPosition,
    to: Square,
    side: Color,
    is_tsuitate: bool,
    scan_file_delta: i8,
    scan_rank_delta: i8,
) -> Option<Square> {
    let mut sq = to.shift(scan_file_delta, scan_rank_delta);
    while let Some(cur) = sq {
        if let Some(piece) = position.piece_at(cur) {
            if piece.color() == side {
                return Some(cur);
            }
            if !is_tsuitate {
                return None;
            }
        }
        sq = cur.shift(scan_file_delta, scan_rank_delta);
    }
    None
}

#[inline(always)]
pub(crate) fn action_index_to_move(
    position: &PartialPosition,
    is_tsuitate: bool,
    action_index: usize,
) -> Option<Move> {
    let side = position.side_to_move();
    let (to, action_kind) = action_to_square_and_kind(action_index)?;

    if action_kind >= DROP_ACTION_OFFSET {
        let piece_kind = *DROP_PIECE_KINDS.get(action_kind - DROP_ACTION_OFFSET)?;
        return Some(Move::Drop {
            piece: Piece::new(piece_kind, side),
            to,
        });
    }

    let promote = action_kind >= PROMOTE_ACTION_OFFSET;
    let dir_idx = if promote {
        action_kind - PROMOTE_ACTION_OFFSET
    } else {
        action_kind
    };
    let (base_df, base_dr) = *MOVE_DELTAS.get(dir_idx)?;
    let sign = if side == Color::White { -1 } else { 1 };
    let move_file_delta = base_df * sign;
    let move_rank_delta = base_dr * sign;
    let scan_file_delta = -move_file_delta;
    let scan_rank_delta = -move_rank_delta;

    let from = if dir_idx >= 8 {
        to.shift(scan_file_delta, scan_rank_delta)?
    } else {
        find_from_on_ray(
            position,
            to,
            side,
            is_tsuitate,
            scan_file_delta,
            scan_rank_delta,
        )?
    };

    Some(Move::Normal { from, to, promote })
}

/// A previously accepted attempt that the game reported as a foul.
/// All entries must belong to the requested player and the current unchanged board.
#[derive(Clone, Copy, Debug)]
pub struct FoulAttempt {
    pub action: Move,
    pub in_check_before: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ActionHistory<'a> {
    pub consecutive_fouls: &'a [FoulAttempt],
    pub last_lost_piece_square: Option<Square>,
}

/// Encode a geometrically valid move; this does not check board legality.
pub fn move_to_action_index(action: Move, side: Color) -> Option<usize> {
    let (to, kind) = match action {
        Move::Drop { piece, to } => {
            if piece.color() != side {
                return None;
            }
            (
                to,
                DROP_ACTION_OFFSET
                    + DROP_PIECE_KINDS
                        .iter()
                        .position(|&k| k == piece.piece_kind())?,
            )
        }
        Move::Normal { from, to, promote } => {
            let sign = if side == Color::Black { 1 } else { -1 };
            let df = (to.file() as i8 - from.file() as i8) * sign;
            let dr = (to.rank() as i8 - from.rank() as i8) * sign;
            let direction = if dr == -2 && df.abs() == 1 {
                (df, dr)
            } else if (df != 0 || dr != 0) && (df == 0 || dr == 0 || df.abs() == dr.abs()) {
                (df.signum(), dr.signum())
            } else {
                return None;
            };
            let dir = MOVE_DELTAS.iter().position(|&d| d == direction)?;
            (to, dir + if promote { PROMOTE_ACTION_OFFSET } else { 0 })
        }
    };
    Some(((to.file() as usize - 1) * 9 + to.rank() as usize - 1) * ACTIONS_PER_SQUARE + kind)
}

pub(crate) fn legal_action_indices_for_position(
    position: &PartialPosition,
    setting: &Setting,
    history: Option<&ActionHistory<'_>>,
) -> Vec<usize> {
    let side = position.side_to_move();
    let square_index = |sq: Square| (sq.file() as usize - 1) * 9 + sq.rank() as usize - 1;
    let mut banned_moves = [Bitboard::empty(); 81];
    let mut banned_drops = [Bitboard::empty(); 7];
    if setting.is_tsuitate {
        if let Some(history) = history {
            if let Some(to) = history.last_lost_piece_square {
                for mask in &mut banned_drops {
                    *mask |= Bitboard::single(to);
                }
            }
            for attempt in history.consecutive_fouls {
                if !is_valid(position, attempt.action, setting) {
                    continue;
                }
                match attempt.action {
                    Move::Drop { piece, to } => {
                        if let Some(k) = DROP_PIECE_KINDS
                            .iter()
                            .position(|&k| k == piece.piece_kind())
                        {
                            banned_drops[k] |= Bitboard::single(to);
                            if setting.game_kind == GameKind::Shogi
                                && piece.piece_kind() != PieceKind::Pawn
                            // A pawn drop may be a foul due to pawn-drop mate, while other pieces may still be droppable on that square.
                            {
                                for mask in &mut banned_drops {
                                    *mask |= Bitboard::single(to);
                                }
                            }
                        }
                    }
                    Move::Normal { from, to, .. } => {
                        let mask = &mut banned_moves[square_index(from)];
                        *mask |= Bitboard::single(to); // Both promotion choices.
                        if attempt.in_check_before != Some(false) {
                            continue;
                        }
                        let df = to.file() as i8 - from.file() as i8;
                        let dr = to.rank() as i8 - from.rank() as i8;
                        let kind = position.piece_at(from).unwrap().piece_kind();
                        let sliding = match kind {
                            PieceKind::Rook | PieceKind::ProRook => df == 0 || dr == 0,
                            PieceKind::Bishop | PieceKind::ProBishop => df.abs() == dr.abs(),
                            PieceKind::Lance => df == 0,
                            _ => false,
                        };
                        if sliding {
                            let mut sq = to.shift(df.signum(), dr.signum());
                            while let Some(to) = sq {
                                *mask |= Bitboard::single(to);
                                sq = to.shift(df.signum(), dr.signum());
                            }
                        }
                    }
                }
            }
        }
    }
    // Generate from existing pieces, encode directly, then emit in index order.
    // Each bitset is a mask of the 27 possible actions for that destination square.
    let mut masks = [0u32; 81];
    let mut own = position.player_bitboard(side) & setting.board_mask;
    while let Some(from) = own.pop() {
        let (plain, promoted) = normal_move_candidates(position, from, setting);
        let mut targets = (plain | promoted) & !banned_moves[square_index(from)];
        while let Some(to) = targets.pop() {
            let index = move_to_action_index(
                Move::Normal {
                    from,
                    to,
                    promote: false,
                },
                side,
            )
            .expect("valid candidate must encode");
            let mask = &mut masks[index / ACTIONS_PER_SQUARE];
            let bit = 1 << (index % ACTIONS_PER_SQUARE);
            if plain.contains(to) {
                *mask |= bit;
            }
            if promoted.contains(to) {
                *mask |= bit << PROMOTE_ACTION_OFFSET;
            }
        }
    }
    for (k, &kind) in DROP_PIECE_KINDS.iter().enumerate() {
        let piece = Piece::new(kind, side);
        if position.hand(piece).unwrap_or(0) == 0 {
            continue;
        }
        let mut targets = drop_candidates(position, piece, setting) & !banned_drops[k];
        while let Some(to) = targets.pop() {
            masks[square_index(to)] |= 1 << (DROP_ACTION_OFFSET + k);
        }
    }
    let mut result = Vec::with_capacity(masks.iter().map(|m| m.count_ones() as usize).sum());
    for (square, mut mask) in masks.into_iter().enumerate() {
        while mask != 0 {
            result.push(square * ACTIONS_PER_SQUARE + mask.trailing_zeros() as usize);
            mask &= mask - 1;
        }
    }
    result
}

pub(crate) fn move_action_indices_to_square(file: u8, rank: u8) -> Vec<usize> {
    if Square::new(file, rank).is_none() {
        return Vec::new();
    }
    let base = ((file as usize - 1) * 9 + (rank as usize - 1)) * ACTIONS_PER_SQUARE;
    (0..DROP_ACTION_OFFSET)
        .map(|action_kind| base + action_kind)
        .collect()
}

pub(crate) fn fill_legal_actions_mask(game: &GameApi, legal_actions: &mut [u8]) {
    legal_actions.fill(0);
    let position = game.position();
    let setting = game.setting();
    for index in legal_action_indices_for_position(position, setting, None) {
        legal_actions[index] = 1;
    }
}

#[inline(always)]
fn observation_channel_index(side_index: usize, channel_index: usize) -> usize {
    side_index * OBSERVATION_CHANNELS_PER_SIDE + channel_index
}

#[inline(always)]
fn set_observation_channel(
    observations: &mut [Bitboard],
    side_index: usize,
    channel_index: usize,
    bitboard: Bitboard,
) {
    let idx = observation_channel_index(side_index, channel_index);
    observations[idx] = bitboard;
}

#[inline(always)]
fn set_observation_square(
    observations: &mut [Bitboard],
    side_index: usize,
    channel_index: usize,
    square: Square,
) {
    let idx = observation_channel_index(side_index, channel_index);
    observations[idx] |= Bitboard::single(square);
}

#[inline(always)]
fn move_to_square(mv: &Move) -> Square {
    match mv {
        Move::Normal { to, .. } | Move::Drop { to, .. } => *to,
    }
}

pub(crate) fn infer_last_move_color(game: &GameApi, mv: &Move) -> Color {
    match mv {
        Move::Drop { piece, .. } => piece.color(),
        Move::Normal { .. } => {
            let side_to_move = game.position().side_to_move();
            match game.last_info() {
                Some(Info::Foul | Info::FoulUnderCheck | Info::LossByFoul) => side_to_move,
                _ => side_to_move.flip(),
            }
        }
    }
}

pub(crate) fn fill_observations(game: &GameApi, observations: &mut [Bitboard]) {
    debug_assert_eq!(observations.len(), OBSERVATION_CHANNEL_COUNT);
    observations.fill(Bitboard::empty());

    let position = game.position();
    let full_bitboard = game.setting().board_mask;
    let last_move = game.last_move();
    let last_move_color = last_move.as_ref().map(|mv| infer_last_move_color(game, mv));
    let last_capture = game.last_capture_piece_kind();
    let last_info = game.last_info();

    for (side_index, side) in OBSERVATION_SIDES.iter().enumerate() {
        for (channel_index, piece_kind) in BOARD_OBSERVATION_PIECE_KINDS.iter().enumerate() {
            set_observation_channel(
                observations,
                side_index,
                OBS_BOARD_CHANNEL_OFFSET + channel_index,
                position.piece_bitboard(Piece::new(*piece_kind, *side)),
            );
        }

        let mut hand_channel_offset = OBS_HAND_CHANNEL_OFFSET;
        for (piece_kind, channel_count) in HAND_OBSERVATION_SPECS {
            let held = position
                .hand(Piece::new(piece_kind, *side))
                .unwrap_or(0)
                .min(channel_count as u8) as usize;
            for nth in 0..channel_count {
                set_observation_channel(
                    observations,
                    side_index,
                    hand_channel_offset + nth,
                    if nth < held {
                        full_bitboard
                    } else {
                        Bitboard::empty()
                    },
                );
            }
            hand_channel_offset += channel_count;
        }

        let last_move_visible = last_move_color.is_some_and(|color| color == *side);
        if last_move_visible {
            if let Some(Move::Normal { from, .. }) = last_move.as_ref() {
                set_observation_square(
                    observations,
                    side_index,
                    OBS_LAST_MOVE_FROM_MOVE_CHANNEL,
                    *from,
                );
            }
        }

        for (channel_index, piece_kind) in DROP_PIECE_KINDS.iter().enumerate() {
            if last_move_visible {
                if let Some(Move::Drop { piece, .. }) = last_move.as_ref() {
                    if piece.piece_kind() == *piece_kind {
                        set_observation_channel(
                            observations,
                            side_index,
                            OBS_LAST_MOVE_FROM_DROP_OFFSET + channel_index,
                            full_bitboard,
                        );
                    }
                }
            }
        }

        if last_move_visible {
            if let Some(mv) = last_move.as_ref() {
                set_observation_square(
                    observations,
                    side_index,
                    OBS_LAST_MOVE_TO_CHANNEL,
                    move_to_square(mv),
                );
            }
        }

        for (channel_index, piece_kind) in DROP_PIECE_KINDS.iter().enumerate() {
            if last_capture == Some(*piece_kind) {
                set_observation_channel(
                    observations,
                    side_index,
                    OBS_LAST_CAPTURE_KIND_OFFSET + channel_index,
                    full_bitboard,
                );
            }
        }

        if last_capture.is_some() {
            if let Some(mv) = last_move.as_ref() {
                set_observation_square(
                    observations,
                    side_index,
                    OBS_LAST_CAPTURE_POSITION_CHANNEL,
                    move_to_square(mv),
                );
            }
        }

        if last_info == Some(Info::None) {
            set_observation_channel(
                observations,
                side_index,
                OBS_LAST_INFO_OFFSET,
                full_bitboard,
            );
        }
        if last_info == Some(Info::Foul) {
            set_observation_channel(
                observations,
                side_index,
                OBS_LAST_INFO_OFFSET + 1,
                full_bitboard,
            );
        }
        if last_info == Some(Info::FoulUnderCheck) {
            set_observation_channel(
                observations,
                side_index,
                OBS_LAST_INFO_OFFSET + 2,
                full_bitboard,
            );
        }
        if last_info == Some(Info::Check) {
            set_observation_channel(
                observations,
                side_index,
                OBS_LAST_INFO_OFFSET + 3,
                full_bitboard,
            );
        }

        if last_move_color == Some(Color::Black) {
            set_observation_channel(observations, side_index, OBS_COLOR_CHANNEL, full_bitboard);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_legality_extended::GameKind;
    use tsuitate_game::csa_to_move;

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

    fn observation_at(
        observations: &[Bitboard],
        side_index: usize,
        channel_index: usize,
        square: Square,
    ) -> u8 {
        u8::from(
            observations[observation_channel_index(side_index, channel_index)].contains(square),
        )
    }

    fn assert_channel_all(
        observations: &[Bitboard],
        expected_full: Bitboard,
        side_index: usize,
        channel_index: usize,
        expected: u8,
    ) {
        let channel = observations[observation_channel_index(side_index, channel_index)];
        if expected == 0 {
            assert!(channel.is_empty());
        } else {
            assert_eq!(channel, expected_full);
        }
    }

    #[test]
    fn observation_count_is_spec_size() {
        assert_eq!(OBSERVATION_CHANNEL_COUNT * 81, 11988);
        assert_eq!(OBSERVATION_BYTES, 2368);
        assert_eq!(OBS_COLOR_CHANNEL + 1, OBSERVATION_CHANNELS_PER_SIDE);
    }

    #[test]
    fn observations_hand_channels_use_binary_stacking() {
        let game = GameApi::new(
            "sfen 9/9/9/9/9/9/9/9/9 b 7P 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();
        let mut observations = [Bitboard::empty(); OBSERVATION_CHANNEL_COUNT];
        fill_observations(&game, &mut observations);
        let full = game.setting().board_mask;

        for ch in 0..18 {
            let expected = if ch < 7 { 1 } else { 0 };
            assert_channel_all(
                &observations,
                full,
                0,
                OBS_HAND_CHANNEL_OFFSET + ch,
                expected,
            );
            assert_channel_all(&observations, full, 1, OBS_HAND_CHANNEL_OFFSET + ch, 0);
        }

        for ch in 18..HAND_OBSERVATION_BINARY_CHANNELS {
            assert_channel_all(&observations, full, 0, OBS_HAND_CHANNEL_OFFSET + ch, 0);
            assert_channel_all(&observations, full, 1, OBS_HAND_CHANNEL_OFFSET + ch, 0);
        }
    }

    #[test]
    fn observations_initial_state_respects_visibility() {
        let game = new_standard_game();
        let mut observations = [Bitboard::empty(); OBSERVATION_CHANNEL_COUNT];
        fill_observations(&game, &mut observations);
        let full = game.setting().board_mask;

        let black_pawn = Square::new(7, 7).unwrap();
        let white_pawn = Square::new(3, 3).unwrap();

        assert_eq!(
            observation_at(&observations, 0, OBS_BOARD_CHANNEL_OFFSET, black_pawn),
            1
        );
        assert_eq!(
            observation_at(&observations, 1, OBS_BOARD_CHANNEL_OFFSET, black_pawn),
            0
        );
        assert_eq!(
            observation_at(&observations, 0, OBS_BOARD_CHANNEL_OFFSET, white_pawn),
            0
        );
        assert_eq!(
            observation_at(&observations, 1, OBS_BOARD_CHANNEL_OFFSET, white_pawn),
            1
        );

        assert_channel_all(&observations, full, 0, OBS_LAST_MOVE_FROM_MOVE_CHANNEL, 0);
        assert_channel_all(&observations, full, 1, OBS_LAST_MOVE_FROM_MOVE_CHANNEL, 0);
        assert_channel_all(&observations, full, 0, OBS_LAST_MOVE_TO_CHANNEL, 0);
        assert_channel_all(&observations, full, 1, OBS_LAST_MOVE_TO_CHANNEL, 0);
        assert_channel_all(&observations, full, 0, OBS_COLOR_CHANNEL, 0);
        assert_channel_all(&observations, full, 1, OBS_COLOR_CHANNEL, 0);
    }

    #[test]
    fn observations_after_black_move_show_only_black_last_move() {
        let mut game = new_standard_game();
        assert!(game.make_move("+7776FU"));

        let mut observations = [Bitboard::empty(); OBSERVATION_CHANNEL_COUNT];
        fill_observations(&game, &mut observations);
        let full = game.setting().board_mask;

        let from = Square::new(7, 7).unwrap();
        let to = Square::new(7, 6).unwrap();

        assert_eq!(
            observation_at(&observations, 0, OBS_LAST_MOVE_FROM_MOVE_CHANNEL, from),
            1
        );
        assert_eq!(
            observation_at(&observations, 1, OBS_LAST_MOVE_FROM_MOVE_CHANNEL, from),
            0
        );
        assert_eq!(
            observation_at(&observations, 0, OBS_LAST_MOVE_TO_CHANNEL, to),
            1
        );
        assert_eq!(
            observation_at(&observations, 1, OBS_LAST_MOVE_TO_CHANNEL, to),
            0
        );

        assert_channel_all(&observations, full, 0, OBS_LAST_INFO_OFFSET, 1);
        assert_channel_all(&observations, full, 1, OBS_LAST_INFO_OFFSET, 1);
        assert_channel_all(&observations, full, 0, OBS_COLOR_CHANNEL, 1);
        assert_channel_all(&observations, full, 1, OBS_COLOR_CHANNEL, 1);
    }

    #[test]
    fn observations_full_channels_follow_board_mask() {
        let game = GameApi::new(
            "sfen 4k/5/5/5/K4 b P 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        let mut observations = [Bitboard::empty(); OBSERVATION_CHANNEL_COUNT];
        fill_observations(&game, &mut observations);
        let full = game.setting().board_mask;

        assert_channel_all(&observations, full, 0, OBS_HAND_CHANNEL_OFFSET, 1);
        assert_channel_all(&observations, full, 1, OBS_HAND_CHANNEL_OFFSET, 0);
    }

    #[test]
    fn action_index_to_move_decodes_black_pawn_push() {
        let game = new_standard_game();
        let idx = ((7 - 1) * 9 + (6 - 1)) * 27;
        let mv = action_index_to_move(game.position(), false, idx).expect("move should decode");
        assert_eq!(mv, csa_to_move("+7776FU", game.position()).unwrap());
    }

    #[test]
    fn action_index_to_move_decodes_white_pawn_push() {
        let mut game = new_standard_game();
        assert!(game.make_move("+7776FU"));
        let idx = ((3 - 1) * 9 + (4 - 1)) * 27;
        let mv = action_index_to_move(game.position(), false, idx).expect("move should decode");
        assert_eq!(mv, csa_to_move("-3334FU", game.position()).unwrap());
    }

    #[test]
    fn legal_actions_mask_contains_start_position_pawn_push() {
        let game = new_standard_game();
        let mut legal_actions = [0u8; ACTION_COUNT];
        fill_legal_actions_mask(&game, &mut legal_actions);

        let pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27;
        let invalid_drop_to_occupied = ((7 - 1) * 9 + (7 - 1)) * 27 + DROP_ACTION_OFFSET;
        assert_eq!(legal_actions[pawn_push], 1);
        assert_eq!(legal_actions[invalid_drop_to_occupied], 0);
    }

    #[test]
    fn move_action_indices_to_square_returns_non_drop_actions_only() {
        let base = ((7 - 1) * 9 + (6 - 1)) * 27;
        let expected: Vec<usize> = (0..DROP_ACTION_OFFSET).map(|kind| base + kind).collect();

        assert_eq!(move_action_indices_to_square(7, 6), expected);
        assert!(move_action_indices_to_square(0, 6).is_empty());
        assert!(move_action_indices_to_square(7, 10).is_empty());
    }

    #[test]
    fn action_index_to_move_decodes_white_knight_move() {
        // White knight: 8b → 7d
        let game = GameApi::new(
            "4k4/1n7/9/9/9/9/9/9/4K4 w - 1",
            GameKind::Shogi.to_u8(),
            false,
            3,
            9,
            9,
            150,
            None,
        )
        .unwrap();

        // Action for a knight jumping forward-left from White's perspective to 7d.
        let idx = ((7 - 1) * 9 + (4 - 1)) * ACTIONS_PER_SQUARE + 9;

        let mv = action_index_to_move(game.position(), false, idx).expect("move should decode");

        assert_eq!(mv, csa_to_move("-8274KE", game.position()).unwrap());
    }
}

#[cfg(test)]
mod action_generation_tests {
    use super::*;
    use shogi_usi_parser::FromUsi;

    fn mini() -> (PartialPosition, Setting) {
        (
            PartialPosition::from_usi("sfen 4rbsgk/8p/9/4P4/4KGSBR/9/9/9/9 b - 1").unwrap(),
            Setting::new(5, 5, 1, GameKind::Shogi, true),
        )
    }
    fn moves(pos: &PartialPosition, setting: &Setting, history: &ActionHistory) -> Vec<Move> {
        legal_action_indices_for_position(pos, setting, Some(history))
            .into_iter()
            .map(|i| action_index_to_move(pos, setting.is_tsuitate, i).unwrap())
            .collect()
    }
    fn mv(text: &str) -> Move {
        Move::from_usi(text).unwrap()
    }

    #[test]
    fn generated_indices_match_full_scan_and_roundtrip() {
        let (mini, setting) = mini();
        let standard = PartialPosition::from_usi("startpos").unwrap();
        for (initial, setting) in [
            (mini, setting),
            (standard, Setting::new(9, 9, 3, GameKind::Shogi, true)),
        ] {
            for tsuitate in [false, true] {
                for side in [Color::Black, Color::White] {
                    let mut pos = initial.clone();
                    pos.side_to_move_set(side);
                    for kind in DROP_PIECE_KINDS {
                        let hand = pos.hand_of_a_player_mut(side);
                        *hand = hand.added(kind).unwrap();
                    }
                    let setting = Setting {
                        is_tsuitate: tsuitate,
                        ..setting.clone()
                    };
                    for step in 0..12 {
                        let expected: Vec<_> = (0..ACTION_COUNT)
                            .filter(|&i| {
                                action_index_to_move(&pos, setting.is_tsuitate, i)
                                    .is_some_and(|m| is_valid(&pos, m, &setting))
                            })
                            .collect();
                        let generated = legal_action_indices_for_position(&pos, &setting, None);
                        assert_eq!(generated, expected);
                        for &index in &generated {
                            let m = action_index_to_move(&pos, setting.is_tsuitate, index).unwrap();
                            assert_eq!(move_to_action_index(m, pos.side_to_move()), Some(index));
                        }
                        if generated.is_empty() {
                            break;
                        }
                        // Randomly pick a move to make, but in a deterministic way for testing.
                        let index = generated[(step * 37 + 5) % generated.len()];
                        let m = action_index_to_move(&pos, setting.is_tsuitate, index).unwrap();
                        if pos.make_move(m).is_none() {
                            break;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_move_ignores_promotion_and_no_check_extends_only_same_ray() {
        let (mut pos, setting) = mini();
        pos.piece_set(Square::new(1, 1).unwrap(), None);
        pos.piece_set(Square::new(1, 2).unwrap(), None);
        pos.piece_set(Square::new(1, 4).unwrap(), Some(Piece::W_P));
        for check in [Some(false), Some(true), None] {
            let attempts = [FoulAttempt {
                action: mv("1e1c"),
                in_check_before: check,
            }];
            let filtered = moves(
                &pos,
                &setting,
                &ActionHistory {
                    consecutive_fouls: &attempts,
                    ..Default::default()
                },
            );
            assert!(!filtered.contains(&mv("1e1c")));
            assert!(filtered.contains(&mv("1e1d"))); // nearer capture can still work
            assert_eq!(filtered.contains(&mv("1e1a")), check != Some(false));
            assert_eq!(filtered.contains(&mv("1e1a+")), check != Some(false));
        }
        let attempts = [FoulAttempt {
            action: mv("1e1a+"),
            in_check_before: None,
        }];
        let filtered = moves(
            &pos,
            &setting,
            &ActionHistory {
                consecutive_fouls: &attempts,
                ..Default::default()
            },
        );
        assert!(!filtered.contains(&mv("1e1a")));
        assert!(!filtered.contains(&mv("1e1a+")));
    }

    #[test]
    fn drop_inference_excludes_pawn_failure_and_capture_is_not_retained() {
        let (mut pos, setting) = mini();
        for kind in [PieceKind::Pawn, PieceKind::Silver, PieceKind::Gold] {
            let hand = pos.hand_of_a_player_mut(Color::Black);
            *hand = hand.added(kind).unwrap();
        }
        for (failed, ban_others) in [("S*3c", true), ("P*3c", false)] {
            let attempts = [FoulAttempt {
                action: mv(failed),
                in_check_before: None,
            }];
            let filtered = moves(
                &pos,
                &setting,
                &ActionHistory {
                    consecutive_fouls: &attempts,
                    ..Default::default()
                },
            );
            assert!(!filtered.contains(&mv(failed)));
            assert_eq!(filtered.contains(&mv("G*3c")), !ban_others);
        }
        let history = ActionHistory {
            last_lost_piece_square: Square::new(3, 3),
            ..Default::default()
        };
        assert!(!moves(&pos, &setting, &history).contains(&mv("G*3c")));
        assert!(moves(&pos, &setting, &ActionHistory::default()).contains(&mv("G*3c")));
    }

    #[test]
    fn sliding_pruning_covers_bishops_lances_and_both_colors() {
        let (_, setting) = mini();
        for side in [Color::Black, Color::White] {
            for (kind, from, failed, farther, nearer) in [
                (PieceKind::Rook, (3, 5), (3, 3), (3, 2), (3, 4)),
                (PieceKind::Lance, (3, 5), (3, 3), (3, 2), (3, 4)),
                (PieceKind::Bishop, (5, 4), (3, 2), (2, 1), (4, 3)),
            ] {
                let sq = |(file, rank)| {
                    Square::new(
                        if side == Color::Black { file } else { 6 - file },
                        if side == Color::Black { rank } else { 6 - rank },
                    )
                    .unwrap()
                };
                let mut pos = PartialPosition::empty();
                pos.side_to_move_set(side);
                pos.piece_set(sq((1, 5)), Some(Piece::new(PieceKind::King, side)));
                pos.piece_set(sq((1, 1)), Some(Piece::new(PieceKind::King, side.flip())));
                pos.piece_set(sq(from), Some(Piece::new(kind, side)));
                pos.piece_set(sq(nearer), Some(Piece::new(PieceKind::Pawn, side.flip())));
                let action = |to| Move::Normal {
                    from: sq(from),
                    to: sq(to),
                    promote: false,
                };
                let history = [FoulAttempt {
                    action: action(failed),
                    in_check_before: Some(false),
                }];
                let filtered = moves(
                    &pos,
                    &setting,
                    &ActionHistory {
                        consecutive_fouls: &history,
                        ..Default::default()
                    },
                );
                assert!(filtered.contains(&action(nearer)));
                assert!(!filtered.contains(&action(farther)));
                assert!(
                    moves(&pos, &setting, &ActionHistory::default()).contains(&action(farther))
                );
            }
        }
    }
}
