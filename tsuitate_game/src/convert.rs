use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CsaParseError {
    TooShort,
    InvalidColor,
    InvalidDigit { index: usize },
    InvalidSquare { file: u8, rank: u8 },
    MissingPiece { square: Square },
    ColorMismatch { expected: Color, actual: Color },
    UnknownPieceCode(String),
}

pub fn nth_ascii(s: &str, n: usize) -> Option<char> {
    s.as_bytes().get(n).map(|&b| b as char)
}

fn csa_digit(csa: &str, index: usize) -> Result<u8, CsaParseError> {
    nth_ascii(csa, index)
        .and_then(|ch| ch.to_digit(10))
        .map(|digit| digit as u8)
        .ok_or(CsaParseError::InvalidDigit { index })
}

fn csa_square(file: u8, rank: u8) -> Result<Square, CsaParseError> {
    Square::new(file, rank).ok_or(CsaParseError::InvalidSquare { file, rank })
}

pub fn csa_to_move(csa: &str, position: &PartialPosition) -> Result<Move, CsaParseError> {
    if csa.len() < 7 {
        return Err(CsaParseError::TooShort);
    }

    let color = match nth_ascii(csa, 0) {
        Some('+') => Color::Black,
        Some('-') => Color::White,
        _ => return Err(CsaParseError::InvalidColor),
    };
    let file_from = csa_digit(csa, 1)?;
    let rank_from = csa_digit(csa, 2)?;
    let file_to = csa_digit(csa, 3)?;
    let rank_to = csa_digit(csa, 4)?;
    let piece_str = csa.get(5..).ok_or(CsaParseError::TooShort)?;

    let piece_kind = csa_to_piece_kind(piece_str)?;

    if file_from == 0 && rank_from == 0 {
        Ok(Move::Drop {
            piece: Piece::new(piece_kind, color),
            to: csa_square(file_to, rank_to)?,
        })
    } else {
        let square_from = csa_square(file_from, rank_from)?;
        let from_piece = position
            .piece_at(square_from)
            .ok_or(CsaParseError::MissingPiece {
                square: square_from,
            })?;
        if from_piece.color() != color {
            return Err(CsaParseError::ColorMismatch {
                expected: from_piece.color(),
                actual: color,
            });
        }
        Ok(Move::Normal {
            from: square_from,
            to: csa_square(file_to, rank_to)?,
            promote: piece_kind != from_piece.piece_kind(),
        })
    }
}

pub fn csa_to_piece_kind(piece_str: &str) -> Result<PieceKind, CsaParseError> {
    let piece_kind = match piece_str {
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
        _ => return Err(CsaParseError::UnknownPieceCode(piece_str.to_owned())),
    };
    Ok(piece_kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csa_to_move_reports_invalid_input() {
        let position = PartialPosition::startpos();

        assert_eq!(
            csa_to_move("+7776", &position),
            Err(CsaParseError::TooShort)
        );
        assert_eq!(
            csa_to_move("*7776FU", &position),
            Err(CsaParseError::InvalidColor)
        );
        assert_eq!(
            csa_to_move("+7a76FU", &position),
            Err(CsaParseError::InvalidDigit { index: 2 })
        );
        assert_eq!(
            csa_to_move("+0076XX", &position),
            Err(CsaParseError::UnknownPieceCode("XX".to_owned()))
        );
    }

    #[test]
    fn csa_to_move_reports_missing_from_piece() {
        let position = PartialPosition::startpos();
        let square = Square::SQ_5E;

        assert_eq!(
            csa_to_move("+5554FU", &position),
            Err(CsaParseError::MissingPiece { square })
        );
    }

    #[test]
    fn csa_to_move_reports_color_mismatch() {
        let position = PartialPosition::startpos();

        assert_eq!(
            csa_to_move("-7776FU", &position),
            Err(CsaParseError::ColorMismatch {
                expected: Color::Black,
                actual: Color::White,
            })
        );
    }
}
