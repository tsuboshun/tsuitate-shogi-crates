use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square};

pub fn nth_ascii(s: &str, n: usize) -> Option<char> {
    s.as_bytes().get(n).map(|&b| b as char)
}

pub fn csa_to_move(csa: &str, position: &PartialPosition) -> Move {
    assert!(csa.len() >= 6);

    let color = match nth_ascii(csa, 0) {
        Some('+') => Color::Black,
        Some('-') => Color::White,
        _ => panic!("invalid color in CSA string"),
    };
    let file_from = nth_ascii(csa, 1).unwrap().to_digit(10).unwrap() as u8;
    let rank_from = nth_ascii(csa, 2).unwrap().to_digit(10).unwrap() as u8;
    let file_to = nth_ascii(csa, 3).unwrap().to_digit(10).unwrap() as u8;
    let rank_to = nth_ascii(csa, 4).unwrap().to_digit(10).unwrap() as u8;
    let piece_str = &csa[5..];

    let piece_kind = csa_to_piece_kind(piece_str);

    if file_from == 0 && rank_from == 0 {
        Move::Drop {
            piece: Piece::new(piece_kind, color),
            to: Square::new(file_to, rank_to).unwrap(),
        }
    } else {
        let square_from = Square::new(file_from, rank_from).unwrap();
        Move::Normal {
            from: square_from,
            to: Square::new(file_to, rank_to).unwrap(),
            promote: piece_kind != position.piece_at(square_from).unwrap().piece_kind(),
        }
    }
}

pub fn csa_to_piece_kind(piece_str: &str) -> PieceKind {
    match piece_str {
        "FU" => PieceKind::Pawn,
        "KY" => PieceKind::Lance,
        "KE" => PieceKind::Knight,
        "GI" => PieceKind::Silver,
        "KI" => PieceKind::Gold,
        "KA" => PieceKind::Bishop,
        "HI" => PieceKind::Rook,
        "OU" => PieceKind::King,
        "TO" => PieceKind::ProPawn,
        "NY" => PieceKind::ProLance,
        "NK" => PieceKind::ProKnight,
        "NG" => PieceKind::ProSilver,
        "UM" => PieceKind::ProBishop,
        "RY" => PieceKind::ProRook,
        _ => panic!("Unknown piece code: {}", piece_str),
    }
}
