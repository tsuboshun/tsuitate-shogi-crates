//! A set of functions for generating features for reinforcement learning.
//!
//! The features are designed based on the book 山岡忠夫, 加納邦彦『強い将棋ソフトの創りかた』マイナビ出版, 2021.

#![allow(dead_code)]
use crate::game_api::GameApi;
use shogi_core::{Bitboard, Color, Move, PartialPosition, Piece, PieceKind, Square};
use shogi_legality_extended::{Setting, is_valid};
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

pub(crate) fn legal_action_indices_for_position(
    position: &PartialPosition,
    setting: &Setting,
) -> Vec<usize> {
    let mut ret = Vec::new();
    for action_index in 0..ACTION_COUNT {
        let is_legal = action_index_to_move(position, setting.is_tsuitate, action_index)
            .is_some_and(|mv| is_valid(position, mv, setting));
        if is_legal {
            ret.push(action_index);
        }
    }
    ret
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
    for action_index in 0..ACTION_COUNT {
        let is_legal = action_index_to_move(position, setting.is_tsuitate, action_index)
            .is_some_and(|mv| is_valid(position, mv, setting));
        legal_actions[action_index] = u8::from(is_legal);
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
        // 白桂: 8二 → 7四
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

        // 7四への、白視点で左前へ跳ぶ桂馬の action
        let idx = ((7 - 1) * 9 + (4 - 1)) * ACTIONS_PER_SQUARE + 9;

        let mv = action_index_to_move(game.position(), false, idx).expect("move should decode");

        assert_eq!(mv, csa_to_move("-8274KE", game.position()).unwrap());
    }
}
