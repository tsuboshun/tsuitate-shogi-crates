use shogi_core::Bitboard;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameKind {
    Shogi = 1,
    Dobutsu = 2,
}

impl GameKind {
    pub fn from_u8(value: u8) -> Option<GameKind> {
        match value {
            1 => Some(GameKind::Shogi),
            2 => Some(GameKind::Dobutsu),
            _ => None,
        }
    }
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Shogi => 1,
            Self::Dobutsu => 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Setting {
    pub game_kind: GameKind,
    pub is_tsuitate: bool,
    pub files: u8,
    pub ranks: u8,
    pub promotion_ranks: u8,
    pub board_mask: Bitboard,
}

impl Setting {
    pub fn new(
        files: u8,
        ranks: u8,
        promotion_ranks: u8,
        game_kind: GameKind,
        is_tsuitate: bool,
    ) -> Self {
        let mut board_mask = Bitboard::empty();
        for file in 1..=files {
            unsafe {
                board_mask |= Bitboard::from_file_unchecked(file, (1 << ranks) - 1);
            }
        }

        Setting {
            files,
            ranks,
            promotion_ranks,
            game_kind,
            is_tsuitate,
            board_mask,
        }
    }
}
