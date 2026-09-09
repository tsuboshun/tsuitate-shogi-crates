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

#[inline]
fn promotion_options(piece: Piece, from: Square, to: Square, setting: &Setting) -> (bool, bool) {
    let rank = relative_rank(to, piece.color(), setting);
    let kind = piece.piece_kind();
    let allow_plain = !(setting.game_kind == GameKind::Shogi
        && ((rank == 1 && matches!(kind, PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight))
            || (rank == 2 && kind == PieceKind::Knight)));
    let zone = if kind == PieceKind::Knight {
        max(2, setting.promotion_rank)
    } else {
        setting.promotion_rank
    } as i8;
    let allow_promoted = piece.promote().is_some()
        && (relative_rank(from, piece.color(), setting) <= zone || rank <= zone);
    (allow_plain, allow_promoted)
}

/// Destinations satisfying `is_valid` for an own piece, split into
/// (unpromoted, promoted) moves. Does not check king safety.
/// Invalid or off-board origins return empty boards.
pub fn normal_move_candidates(
    position: &PartialPosition,
    from: Square,
    setting: &Setting,
) -> (shogi_core::Bitboard, shogi_core::Bitboard) {
    use shogi_core::Bitboard;
    let mut plain = Bitboard::empty();
    let mut promoted = Bitboard::empty();
    if !setting.board_mask.contains(from) {
        return (plain, promoted);
    }
    let Some(piece) = position
        .piece_at(from)
        .filter(|p| p.color() == position.side_to_move())
    else {
        return (plain, promoted);
    };
    let mut targets = normal::from_candidates(position, piece, from, setting);
    if !setting.is_tsuitate {
        targets &= !position.piece_bitboard(Piece::new(PieceKind::King, piece.color().flip()));
    }
    while let Some(to) = targets.pop() {
        let (allow_plain, allow_promoted) = promotion_options(piece, from, to, setting);
        if allow_plain {
            plain |= Bitboard::single(to);
        }
        if allow_promoted {
            promoted |= Bitboard::single(to);
        }
    }
    (plain, promoted)
}

/// All destinations satisfying `is_valid` for a drop of `piece`.
/// Pawn-drop mate validation is retained for non-tsuitate games.
pub fn drop_candidates(
    position: &PartialPosition,
    piece: Piece,
    setting: &Setting,
) -> shogi_core::Bitboard {
    use shogi_core::Bitboard;
    if piece.color() != position.side_to_move() || position.hand(piece).unwrap_or(0) == 0 {
        return Bitboard::empty();
    }
    let mut targets = setting.board_mask
        & if setting.is_tsuitate {
            !position.player_bitboard(piece.color())
        } else {
            position.vacant_bitboard()
        };
    if setting.game_kind == GameKind::Shogi
        && matches!(
            piece.piece_kind(),
            PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight
        )
    {
        // Build permitted ranks once for the entire piece kind.
        let mut ranks = 0u16;
        for rank in 1..=9 {
            let relative = if piece.color() == Color::Black {
                rank
            } else {
                setting.ranks as i8 - rank + 1
            };
            if !(relative == 1
                && matches!(
                    piece.piece_kind(),
                    PieceKind::Pawn | PieceKind::Lance | PieceKind::Knight
                )
                || relative == 2 && piece.piece_kind() == PieceKind::Knight)
            {
                ranks |= 1 << (rank - 1);
            }
        }
        let mut allowed = Bitboard::empty();
        let pawns = position.piece_bitboard(Piece::new(PieceKind::Pawn, piece.color()));
        for file in 1..=9 {
            // Safety: file is in 1..=9, and rank bits are limited to nine bits.
            let file_mask = unsafe { Bitboard::from_file_unchecked(file, 0x1ff) };
            if piece.piece_kind() != PieceKind::Pawn || (pawns & file_mask).is_empty() {
                allowed |= unsafe { Bitboard::from_file_unchecked(file, ranks) };
            }
        }
        targets &= allowed;
    }
    if !setting.is_tsuitate && piece.piece_kind() == PieceKind::Pawn {
        let mut unchecked = targets;
        while let Some(to) = unchecked.pop() {
            if !is_valid(position, Move::Drop { piece, to }, setting) {
                targets &= !Bitboard::single(to);
            }
        }
    }
    targets
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
            let (plain, promoted) = promotion_options(from_piece, from, to, setting);
            if !(if promote { promoted } else { plain }) {
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
                // A whole file at once, including squares outside a variant's board,
                // matching the original nine-square scan.
                let file = unsafe { shogi_core::Bitboard::from_file_unchecked(to.file(), 0x1ff) };
                if !(position.piece_bitboard(piece) & file).is_empty() {
                    return false;
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
    fn bulk_candidates_match_single_move_validation() {
        for (files, ranks, zone, kind) in [
            (9, 9, 3, GameKind::Shogi),
            (5, 5, 1, GameKind::Shogi),
            (3, 4, 1, GameKind::Dobutsu),
            (5, 5, 0, GameKind::Shogi),
        ] {
            for tsuitate in [false, true] {
                for side in [Color::Black, Color::White] {
                    let mut position = PartialPosition::from_usi(
                        "sfen 4rbsgk/4+n+l+ppp/4+B+R+S+L+N/4PLNSG/4KGSBR/9/9/9/9 b RBGSNLP 1",
                    )
                    .unwrap();
                    position.side_to_move_set(side);
                    *position.hand_of_a_player_mut(side) = position.hand_of_a_player(Color::Black);
                    let setting = Setting::new(files, ranks, zone, kind, tsuitate);
                    for from in Square::all() {
                        let (plain, promoted) = normal_move_candidates(&position, from, &setting);
                        for to in Square::all() {
                            for (promote, candidates) in [(false, plain), (true, promoted)] {
                                assert_eq!(
                                    candidates.contains(to),
                                    is_valid(
                                        &position,
                                        Move::Normal { from, to, promote },
                                        &setting
                                    ),
                                    "{setting:?} {side:?} {from:?} {to:?} {promote}"
                                );
                            }
                        }
                    }
                    for piece in Piece::all() {
                        let candidates = drop_candidates(&position, piece, &setting);
                        for to in Square::all() {
                            assert_eq!(
                                candidates.contains(to),
                                is_valid(&position, Move::Drop { piece, to }, &setting),
                                "{setting:?} {piece:?} {to:?}"
                            );
                        }
                    }
                }
            }
        }
    }

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
