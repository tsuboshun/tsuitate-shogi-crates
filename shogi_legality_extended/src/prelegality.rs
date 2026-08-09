use crate::normal;
use crate::setting::{GameKind, Setting};
use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square};
use std::cmp::max;

fn relative_rank(square: Square, color: Color, setting: &Setting) -> i8 {
    let rank = square.rank();
    match color {
        Color::Black => rank as i8,
        Color::White => setting.ranks as i8 - rank as i8 + 1,
    }
}

/// Checks if a move is valid without considering king's safety.
pub fn is_valid(position: &PartialPosition, mv: Move, setting: &Setting) -> bool {
    let side = position.side_to_move();
    match mv {
        Move::Normal { from, to, promote } => {
            if !setting.board_mask.contains(from) {
                return false;
            }
            // Is `from` occupied by `side`'s piece?
            let from_piece = if let Some(x) = position.piece_at(from) {
                x
            } else {
                return false;
            };
            if from_piece.color() != side {
                return false;
            }
            // Is `to` occupied by `side`'s piece?
            let to_piece = position.piece_at(to);
            if let Some(x) = to_piece {
                if x.color() == side {
                    return false;
                }
                // Capturing king is not allowed.
                if !setting.is_tsuitate && x.piece_kind() == PieceKind::King {
                    return false;
                }
            }
            // Stuck?
            let rel_rank = relative_rank(to, side, &setting);
            if setting.game_kind == GameKind::Shogi {
                if rel_rank == 1
                    && matches!(
                        from_piece.piece_kind(),
                        PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight,
                    )
                    && !promote
                {
                    return false;
                }
                if rel_rank == 2 && from_piece.piece_kind() == PieceKind::Knight && !promote {
                    return false;
                }
            }
            // Can promote?
            let promotion_rank = match from_piece.piece_kind() {
                PieceKind::Knight => max(2, setting.promotion_rank), // Knight is exception
                _ => setting.promotion_rank,
            };
            if promote
                && relative_rank(from, side, &setting) > promotion_rank as i8
                && rel_rank > promotion_rank as i8
            {
                return false;
            }
            if promote && from_piece.promote().is_none() {
                return false;
            }

            // Is the move valid?
            normal::check(position, from_piece, from, to, &setting)
        }
        Move::Drop { piece, to } => {
            // Is the destination within the board?
            if !setting.board_mask.contains(to) {
                return false;
            }
            // Does `side` have a piece?
            if piece.color() != side {
                return false;
            }
            let remaining = if let Some(x) = position.hand(piece) {
                x
            } else {
                return false;
            };
            if remaining == 0 {
                return false;
            }
            // Is `to` vacant?
            if position.piece_at(to).is_some() {
                if !setting.is_tsuitate || position.piece_at(to).unwrap().color() == side {
                    return false;
                }
            }
            // Stuck?
            let rel_rank = relative_rank(to, side, &setting);
            if setting.game_kind == GameKind::Shogi {
                if rel_rank == 1
                    && matches!(
                        piece.piece_kind(),
                        PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight,
                    )
                {
                    return false;
                }
                if rel_rank == 2 && piece.piece_kind() == PieceKind::Knight {
                    return false;
                }
            }
            // Does a double-pawn (`二歩`, *nifu*) happen?
            if piece.piece_kind() == PieceKind::Pawn && setting.game_kind == GameKind::Shogi {
                let file = to.file();
                for i in 1..=9 {
                    let square = unsafe { Square::new(file, i).unwrap_unchecked() };
                    if position.piece_at(square) == Some(piece) {
                        return false;
                    }
                }
            }
            // Does a drop-pawn-mate (`打ち歩詰め`, *uchifu-zume*) happen?
            if !setting.is_tsuitate && piece.piece_kind() == PieceKind::Pawn {
                let mut next = position.clone();
                let result = next.make_move(mv); // always Some(())
                debug_assert_eq!(result, Some(()));
                if is_mate_after_pawn_drop(&next, setting) == Some(true) {
                    return false;
                }
            }
            true
        }
    }
}

fn all_valid_normal_moves<'a>(
    position: &'a PartialPosition,
    setting: &'a Setting,
) -> impl Iterator<Item = Move> + 'a {
    Square::all()
        .flat_map(|from| {
            Square::all().flat_map(move |to| {
                [false, true]
                    .into_iter()
                    .map(move |promote| Move::Normal { from, to, promote })
            })
        })
        .filter(|&mv| is_valid(position, mv, setting))
}

/// Returns all valid moves without considering king's safety.
pub fn all_valid_moves<'a>(
    position: &'a PartialPosition,
    setting: &'a Setting,
) -> impl Iterator<Item = Move> + 'a {
    all_valid_normal_moves(position, setting)
        .chain(
            Piece::all()
                .into_iter()
                .flat_map(|piece| Square::all().map(move |to| Move::Drop { piece, to })),
        )
        .filter(|&mv| is_valid(position, mv, setting))
}

/// Can `side` play a move that captures the opponent's king?
///
/// This function returns None if the opponent has no king.
pub fn will_king_be_captured(
    position: &PartialPosition,
    side: Color,
    game_kind: GameKind,
) -> Option<bool> {
    let occupied = !position.vacant_bitboard();
    let king = position.king_position(side.flip())?;
    let king_file = king.file();
    let king_rank = king.rank();
    let king_peripheral = crate::normal::king(king_file, king_rank);
    let my_bb_peripheral = position.player_bitboard(side) & king_peripheral;
    if !my_bb_peripheral.is_empty() {
        for piece_kind in [PieceKind::King, PieceKind::ProBishop, PieceKind::ProRook] {
            let my_piece = Piece::new(piece_kind, side);
            let piece_bb = position.piece_bitboard(my_piece);
            if !(piece_bb & king_peripheral).is_empty() {
                return Some(true);
            }
        }
        for piece_kind in [
            PieceKind::Pawn,
            PieceKind::Silver,
            PieceKind::Gold,
            PieceKind::ProPawn,
            PieceKind::ProLance,
            PieceKind::ProKnight,
            PieceKind::ProSilver,
        ] {
            let piece = Piece::new(piece_kind, side.flip());
            let my_piece = Piece::new(piece_kind, side);
            let piece_bb = position.piece_bitboard(my_piece);
            let attack = crate::normal::from_candidates_without_assertion(
                occupied, position, piece, king_file, king_rank, game_kind,
            );
            if !(piece_bb & attack).is_empty() {
                return Some(true);
            }
        }
    }
    // lance, knight
    {
        let my_piece = Piece::new(PieceKind::Lance, side);
        let piece_bb = position.piece_bitboard(my_piece);
        if !piece_bb.is_empty() {
            // from `king`, can `piece` reach a piece of `side` with `piece_kind`?
            let attack = crate::normal::lance_range(side.flip(), king_file, king_rank, occupied);
            if !(attack & piece_bb).is_empty() {
                return Some(true);
            }
        }
        let my_piece = Piece::new(PieceKind::Knight, side);
        let piece_bb = position.piece_bitboard(my_piece);
        if !piece_bb.is_empty() {
            // from `king`, can `piece` reach a piece of `side` with `piece_kind`?
            let attack = crate::normal::knight(side.flip(), king_file, king_rank);
            if !(attack & piece_bb).is_empty() {
                return Some(true);
            }
        }
    }
    macro_rules! ranges {
        ($piece_kind:expr, $pro_piece_kind:expr, $func:expr,) => {
            let my_piece = Piece::new($piece_kind, side);
            let my_pro_piece = Piece::new($pro_piece_kind, side);
            let piece_bb =
                position.piece_bitboard(my_piece) | position.piece_bitboard(my_pro_piece);
            if !piece_bb.is_empty() {
                // from `king`, can `piece` reach a piece of `side` with `piece_kind`?
                let attack = $func(king_file, king_rank, occupied);
                if !(attack & piece_bb).is_empty() {
                    return Some(true);
                }
            }
        };
    }
    ranges!(
        PieceKind::Bishop,
        PieceKind::ProBishop,
        crate::normal::bishop_range,
    );
    ranges!(
        PieceKind::Rook,
        PieceKind::ProRook,
        crate::normal::rook_range,
    );
    Some(false)
}

/// Checks whether a pawn drop checkmates the opponent.
///
/// Since a pawn gives check from an adjacent square, a drop cannot block that
/// check. Only normal moves need to be considered as possible responses. This
/// also avoids the `is_valid` -> `is_mate` -> `is_valid` recursion.
fn is_mate_after_pawn_drop(position: &PartialPosition, setting: &Setting) -> Option<bool> {
    position.king_position(position.side_to_move())?;

    if !will_king_be_captured(position, position.side_to_move().flip(), setting.game_kind)? {
        return Some(false);
    }
    for mv in all_valid_normal_moves(position, setting) {
        let mut next = position.clone();
        let result = next.make_move(mv);
        debug_assert_eq!(result, Some(()));
        if !will_king_be_captured(&next, next.side_to_move(), setting.game_kind)? {
            return Some(false);
        }
    }
    Some(true)
}

/// Checks if `side`'s king has no way to escape from being captured.
///
/// This function returns None if `side` has no king.
///
/// For this function to return Some(true), the king does not need to be in check.
///
/// Since: 0.1.2
pub fn is_mate(position: &PartialPosition, setting: &Setting) -> Option<bool> {
    position.king_position(position.side_to_move())?; // Early return if no king.
    let mut response_setting = setting.clone();
    response_setting.is_tsuitate = false;
    let all = all_valid_moves(position, &response_setting);

    if !will_king_be_captured(&position, position.side_to_move().flip(), setting.game_kind)? {
        return Some(false);
    }
    for mv in all {
        let mut next = position.clone();
        let result = next.make_move(mv);
        debug_assert_eq!(result, Some(()));
        if !will_king_be_captured(&next, next.side_to_move(), setting.game_kind)? {
            return Some(false);
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use shogi_usi_parser::FromUsi;

    use super::*;

    #[test]
    fn drop_pawn_0() {
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, false);
        let position =
            PartialPosition::from_usi("sfen 7l1/7pk/7n1/8R/7N1/9/9/9/9 w r2b4g4s2n3l17p 1")
                .unwrap();
        let mv = Move::Drop {
            piece: Piece::new(PieceKind::Pawn, Color::White),
            to: Square::SQ_1C,
        };
        assert!(is_valid(&position, mv, &setting));
    }

    #[test]
    fn drop_pawn_mate_is_invalid() {
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, false);
        let position = PartialPosition::from_usi("sfen 8k/7R1/7G1/9/9/9/9/9/K8 b P 1").unwrap();
        let mv = Move::Drop {
            piece: Piece::new(PieceKind::Pawn, Color::Black),
            to: Square::SQ_1B,
        };

        assert!(!is_valid(&position, mv, &setting));
    }

    #[test]
    fn mate_after_3957_pro_bishop_does_not_recurse_through_pawn_drops() {
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, true);
        let mut position = PartialPosition::from_usi(
            "sfen 6knl/4G4/3pgppSp/3+b1n+S2/+SN5P1/l1PKN2G1/1P1PP3P/3g3+sL/2+r1r1b2 w L5P2p 112",
        )
        .unwrap();
        let mv = Move::Normal {
            from: Square::SQ_3I,
            to: Square::SQ_5G,
            promote: false,
        };
        position.make_move(mv).unwrap();

        assert_eq!(is_mate(&position, &setting), Some(true));
    }

    #[test]
    fn mate_responses_use_non_tsuitate_legality() {
        let setting = Setting::new(1, 2, 1, GameKind::Shogi, true);
        let position = PartialPosition::from_usi("sfen 8k/8K/9/9/9/9/9/9/9 b - 1").unwrap();

        assert_eq!(is_mate(&position, &setting), Some(true));
    }
}
