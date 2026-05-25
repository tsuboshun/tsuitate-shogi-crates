use shogi_core::{Color, Move, PartialPosition, PieceKind};
use shogi_legality_extended::{Setting, is_mate, is_valid, will_king_be_captured};

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum Info {
    None = 0,
    Foul = 1,
    FoulUnderCheck = 2,
    Check = 3,
    Checkmate = 4,
    LossByFoul = 5,
    Draw = 6,
}

#[derive(Eq, PartialEq, Clone, Debug, Default)]
pub struct Game {
    pub inner: PartialPosition,
    pub fouls: [i8; 2],
    pub draw_move_count: u16,
    pub last_move: Option<Move>,
    pub last_info: Option<Info>,
    pub last_capture: Option<PieceKind>,
}

impl Game {
    pub fn new(
        initial: PartialPosition,
        fouls: [i8; 2],
        draw_move_count: u16,
        last_info: Option<Info>,
    ) -> Self {
        Self {
            inner: initial,
            fouls,
            draw_move_count,
            last_move: None,
            last_info,
            last_capture: None,
        }
    }

    pub fn make_move(&mut self, mv: Move, setting: &Setting) -> Option<()> {
        if self.fouls[0] < 0
            || self.fouls[1] < 0
            || self.inner.ply() > self.draw_move_count
            || self.last_info == Some(Info::Checkmate)
        {
            return None;
        }
        if !is_valid(&self.inner, mv, &setting) {
            return None;
        }
        if setting.is_tsuitate
            && !is_valid(
                &self.inner,
                mv,
                &Setting {
                    is_tsuitate: false,
                    ..*setting
                },
            )
        {
            // a foul specific to tsuitate
            let mut info = match self.last_info {
                Some(Info::Check) | Some(Info::FoulUnderCheck) => Info::FoulUnderCheck,
                _ => Info::Foul,
            };
            match self.inner.side_to_move() {
                Color::Black => {
                    self.fouls[0] -= 1;
                }
                Color::White => {
                    self.fouls[1] -= 1;
                }
            }
            if self.fouls[0] < 0 || self.fouls[1] < 0 {
                info = Info::LossByFoul;
            }
            self.last_move = Some(mv);
            self.last_info = Some(info);
            self.last_capture = None;
            return Some(());
        }
        // tentative move
        let mut partial_pos = self.inner.clone();
        partial_pos.make_move(mv)?;

        let is_my_king_in_check = match will_king_be_captured(
            &partial_pos,
            partial_pos.side_to_move(),
            setting.game_kind,
        ) {
            Some(value) => value,
            None => false, // when my king does not exist (this is possible in Tsume Shogi)
        };
        if is_my_king_in_check {
            // if player's king is still in check after making the move, it's a foul
            if !setting.is_tsuitate {
                // in normal shogi, do nothing
                return None;
            }
            let mut info = match self.last_info {
                Some(Info::Check) | Some(Info::FoulUnderCheck) => Info::FoulUnderCheck,
                None => {
                    // if the initial position of this game is in a state of check, self.last_info is None
                    // but we should treat it as FoulUnderCheck
                    match will_king_be_captured(
                        &self.inner,
                        self.inner.side_to_move().flip(),
                        setting.game_kind,
                    ) {
                        Some(true) => Info::FoulUnderCheck,
                        _ => Info::Foul,
                    }
                }
                _ => Info::Foul,
            };
            match self.inner.side_to_move() {
                Color::Black => {
                    self.fouls[0] -= 1;
                }
                Color::White => {
                    self.fouls[1] -= 1;
                }
            }
            if self.fouls[0] < 0 || self.fouls[1] < 0 {
                info = Info::LossByFoul;
            }
            self.last_move = Some(mv);
            self.last_info = Some(info);
            self.last_capture = None;
            return Some(());
        } else {
            let last_capture = match mv {
                Move::Normal {
                    from: _,
                    to,
                    promote: _,
                } => self.inner.piece_at(to).map(|piece| {
                    let obtaining = piece.piece_kind();
                    if let Some(piece_kind) = obtaining.unpromote() {
                        piece_kind
                    } else {
                        obtaining
                    }
                }),
                _ => None,
            };
            // checked or checkmated opponent's king?
            self.inner = partial_pos;
            self.last_move = Some(mv);
            self.last_capture = last_capture;
            let is_opponent_king_in_check = match will_king_be_captured(
                &self.inner,
                self.inner.side_to_move().flip(),
                setting.game_kind,
            ) {
                Some(value) => value,
                None => {
                    // when opponent king does not exist (this is possible in Tsume Shogi)
                    self.last_info = Some(Info::None);
                    return Some(());
                }
            };
            let mut info = if is_opponent_king_in_check {
                let mate = is_mate(&self.inner, &setting)?;
                if mate { Info::Checkmate } else { Info::Check }
            } else {
                Info::None
            };
            if info != Info::Checkmate && self.inner.ply() > self.draw_move_count {
                info = Info::Draw;
            }
            self.last_info = Some(info);
            return Some(());
        }
    }

    pub fn to_sfen_owned(&self) -> String {
        self.inner.to_sfen_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::csa_to_move;
    use shogi_legality_extended::{GameKind, is_mate};
    use shogi_usi_parser::FromUsi;

    #[test]
    fn test_foul_move() {
        let mut game = Game::new(
            PartialPosition::from_usi(
                "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            )
            .unwrap(),
            [9, 9],
            150,
            None,
        );
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, true);
        game.make_move(csa_to_move("+7776FU", &game.inner).unwrap(), &setting);
        game.make_move(csa_to_move("-3334FU", &game.inner).unwrap(), &setting);
        game.make_move(csa_to_move("+5968OU", &game.inner).unwrap(), &setting);
        game.make_move(csa_to_move("-5142OU", &game.inner).unwrap(), &setting);
        game.make_move(csa_to_move("+6877OU", &game.inner).unwrap(), &setting);

        assert_eq!(game.last_info, Some(Info::Foul));
        assert_eq!(game.fouls, [8, 9]);
    }

    #[test]
    fn test_drop_foul_move() {
        let mut game = Game::new(
            PartialPosition::from_usi("sfen 6gks/7p1/7P1/6SKG/9/9/9/9/9 b - 1").unwrap(),
            [9, 9],
            150,
            None,
        );
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, false);
        game.make_move(csa_to_move("+2322FU", &game.inner).unwrap(), &setting);
        game.make_move(csa_to_move("-1122GI", &game.inner).unwrap(), &setting);
        let result = game.make_move(csa_to_move("+0022FU", &game.inner).unwrap(), &setting);
        assert_eq!(result, None);
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, true);
        let result = game.make_move(csa_to_move("+0022FU", &game.inner).unwrap(), &setting);
        assert_eq!(result, Some(()));
        assert_eq!(game.last_info, Some(Info::Foul));
    }

    #[test]
    fn test_smaller_board() {
        let game = Game::new(
            PartialPosition::from_usi("sfen 7gg/7K1/9/9/9/9/9/9/9 b - 1").unwrap(),
            [9, 9],
            150,
            None,
        );
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, true);
        assert_eq!(is_mate(&game.inner, &setting), Some(false));
        let setting = Setting::new(2, 2, 1, GameKind::Shogi, true); // it is mate when board size is 2 x 2
        assert_eq!(is_mate(&game.inner, &setting), Some(true));
    }

    #[test]
    fn test_dobutsu() {
        let mut game = Game::new(
            PartialPosition::from_usi("sfen 6gks/7p1/7P1/6SKG/9/9/9/9/9 b - 1").unwrap(),
            [9, 9],
            150,
            None,
        );
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, true);
        let result = game.make_move(csa_to_move("+3433GI", &game.inner).unwrap(), &setting);
        assert_eq!(result, None); // in dobutsu, GI represents Elephant, which only moves diagonally
        let setting = Setting::new(3, 4, 1, GameKind::Shogi, true);
        let result = game.make_move(csa_to_move("+3433GI", &game.inner).unwrap(), &setting);
        assert_eq!(result, Some(()));
    }
}
