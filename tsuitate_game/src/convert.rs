use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square, ToUsi};

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

pub fn csa_coord_digit(csa: &str, index: usize) -> Option<u8> {
    nth_ascii(csa, index)
        .and_then(|ch| ch.to_digit(10))
        .map(|digit| digit as u8)
}

fn csa_digit(csa: &str, index: usize) -> Result<u8, CsaParseError> {
    csa_coord_digit(csa, index).ok_or(CsaParseError::InvalidDigit { index })
}

fn csa_square(file: u8, rank: u8) -> Result<Square, CsaParseError> {
    Square::new(file, rank).ok_or(CsaParseError::InvalidSquare { file, rank })
}

pub fn color_to_csa_sign(color: Color) -> char {
    match color {
        Color::Black => '+',
        Color::White => '-',
    }
}

pub fn csa_sign_to_color(sign: char) -> Option<Color> {
    match sign {
        '+' => Some(Color::Black),
        '-' => Some(Color::White),
        _ => None,
    }
}

pub fn csa_color_to_color(csa_color: &str) -> Option<Color> {
    if csa_color.len() != 1 {
        return None;
    }
    nth_ascii(csa_color, 0).and_then(csa_sign_to_color)
}

pub fn piece_kind_to_csa(piece_kind: PieceKind) -> &'static str {
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

pub fn piece_to_sfen(piece: Piece) -> String {
    let mut buf = String::new();
    piece.to_usi(&mut buf).unwrap();
    buf
}

pub fn piece_kind_to_sfen(piece_kind: PieceKind) -> String {
    let mut buf = String::new();
    piece_kind.to_usi(&mut buf).unwrap();
    buf
}

pub fn csa_to_move(csa: &str, position: &PartialPosition) -> Result<Move, CsaParseError> {
    if csa.len() < 7 {
        return Err(CsaParseError::TooShort);
    }

    let color = nth_ascii(csa, 0)
        .and_then(csa_sign_to_color)
        .ok_or(CsaParseError::InvalidColor)?;
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

/// Converts a move to CSA notation using the position immediately before or
/// after the move.
pub fn move_to_csa(mv: Move, position: &PartialPosition) -> Option<String> {
    match mv {
        Move::Drop { piece, to } => {
            let sign = color_to_csa_sign(piece.color());
            let piece_csa = piece_kind_to_csa(piece.piece_kind());
            Some(format!("{}00{}{}{}", sign, to.file(), to.rank(), piece_csa))
        }
        Move::Normal { from, to, promote } => {
            let (piece, piece_kind) = if let Some(piece) = position.piece_at(from) {
                let piece_kind = if promote {
                    piece.piece_kind().promote()?
                } else {
                    piece.piece_kind()
                };
                (piece, piece_kind)
            } else {
                let piece = position.piece_at(to)?;
                (piece, piece.piece_kind())
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
    fn csa_color_to_color_requires_one_sign() {
        assert_eq!(csa_color_to_color("+"), Some(Color::Black));
        assert_eq!(csa_color_to_color("-"), Some(Color::White));
        assert_eq!(csa_color_to_color(""), None);
        assert_eq!(csa_color_to_color("++"), None);
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

    #[test]
    fn move_to_csa_accepts_position_after_move() {
        let mut position = PartialPosition::startpos();
        let mv = Move::Normal {
            from: Square::SQ_7G,
            to: Square::SQ_7F,
            promote: false,
        };
        position.make_move(mv).unwrap();

        assert_eq!(move_to_csa(mv, &position).as_deref(), Some("+7776FU"));
    }
}
