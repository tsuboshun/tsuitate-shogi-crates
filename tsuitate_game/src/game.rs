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
    initial: PartialPosition,
    pub inner: PartialPosition,
    pub fouls: [i8; 2],
    pub draw_move_count: u16,
    pub moves: Vec<Move>,
    pub infos: Vec<Info>,
    pub captures: Vec<Option<PieceKind>>, // it is a kind of info, but treat it separately for simplicity
}

impl Game {
    pub fn new(initial: PartialPosition, fouls: [i8; 2], draw_move_count: u16) -> Self {
        Self {
            initial: initial.clone(),
            inner: initial,
            fouls,
            draw_move_count,
            moves: Vec::new(),
            infos: Vec::new(),
            captures: Vec::new(),
        }
    }

    pub fn make_move(&mut self, mv: Move, setting: &Setting) -> Option<()> {
        if self.fouls[0] < 0 || self.fouls[1] < 0 || self.inner.ply() > self.draw_move_count {
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
            self.moves.push(mv);
            let mut info = match self.infos.last() {
                Some(Info::Check) | Some(Info::FoulUnderCheck) => Info::FoulUnderCheck,
                _ => Info::Foul,
            };
            self.captures.push(None);
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
            self.infos.push(info);
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
            self.moves.push(mv);
            let mut info = match self.infos.last() {
                Some(Info::Check) | Some(Info::FoulUnderCheck) => Info::FoulUnderCheck,
                None => {
                    // if the initial position of this game is in a state of check, self.infos.last() is None
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
            self.captures.push(None);
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
            self.infos.push(info);
            return Some(());
        } else {
            // captured piece?
            match mv {
                Move::Normal {
                    from: _,
                    to,
                    promote: _,
                } => {
                    if let Some(piece) = self.inner.piece_at(to) {
                        let obtaining = piece.piece_kind();
                        let captured = if let Some(piece_kind) = obtaining.unpromote() {
                            piece_kind
                        } else {
                            obtaining
                        };
                        self.captures.push(Some(captured));
                    } else {
                        self.captures.push(None);
                    }
                }
                _ => {
                    self.captures.push(None);
                }
            };
            // checked or checkmated opponent's king?
            self.inner = partial_pos;
            self.moves.push(mv);
            let is_opponent_king_in_check = match will_king_be_captured(
                &self.inner,
                self.inner.side_to_move().flip(),
                setting.game_kind,
            ) {
                Some(value) => value,
                None => {
                    // when opponent king does not exist (this is possible in Tsume Shogi)
                    self.infos.push(Info::None);
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
            self.infos.push(info);
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
        );
        let setting = Setting::new(9, 9, 3, GameKind::Shogi, true);
        game.make_move(csa_to_move("+7776FU", &game.inner), &setting);
        game.make_move(csa_to_move("-3334FU", &game.inner), &setting);
        game.make_move(csa_to_move("+5968OU", &game.inner), &setting);
        game.make_move(csa_to_move("-5142OU", &game.inner), &setting);
        game.make_move(csa_to_move("+6877OU", &game.inner), &setting);

        assert_eq!(
            game.infos,
            vec![Info::None, Info::None, Info::None, Info::None, Info::Foul]
        );
        assert_eq!(game.fouls, [8, 9]);
    }

    #[test]
    fn test_drop_foul_move() {
        let mut game = Game::new(
            PartialPosition::from_usi("sfen 6gks/7p1/7P1/6SKG/9/9/9/9/9 b - 1").unwrap(),
            [9, 9],
            150,
        );
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, false);
        game.make_move(csa_to_move("+2322FU", &game.inner), &setting);
        game.make_move(csa_to_move("-1122GI", &game.inner), &setting);
        let result = game.make_move(csa_to_move("+0022FU", &game.inner), &setting);
        assert_eq!(result, None);
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, true);
        let result = game.make_move(csa_to_move("+0022FU", &game.inner), &setting);
        assert_eq!(result, Some(()));
        assert_eq!(game.infos, vec![Info::Check, Info::None, Info::Foul]);
    }

    #[test]
    fn test_smaller_board() {
        let game = Game::new(
            PartialPosition::from_usi("sfen 7gg/7K1/9/9/9/9/9/9/9 b - 1").unwrap(),
            [9, 9],
            150,
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
        );
        let setting = Setting::new(3, 4, 1, GameKind::Dobutsu, true);
        let result = game.make_move(csa_to_move("+3433GI", &game.inner), &setting);
        assert_eq!(result, None); // in dobutsu, GI represents Elephant, which only moves diagonally
        let setting = Setting::new(3, 4, 1, GameKind::Shogi, true);
        let result = game.make_move(csa_to_move("+3433GI", &game.inner), &setting);
        assert_eq!(result, Some(()));
    }
}
