use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use shogi_core::Color;
use tsuitate_game::{Info, color_to_csa_sign, csa_color_to_color};

use crate::game_api::GameApi;

fn parse_optional_csa_color(csa_color: Option<&str>) -> PyResult<Option<Color>> {
    match csa_color {
        Some(value) => csa_color_to_color(value)
            .map(Some)
            .ok_or_else(|| PyValueError::new_err("invalid color")),
        None => Ok(None),
    }
}

#[pyclass(name = "Game")]
pub(crate) struct PyGame {
    inner: GameApi,
}

#[pymethods]
impl PyGame {
    #[new]
    #[pyo3(signature = (
        sfen,
        game_kind,
        is_tsuitate,
        promotion_rank,
        foul0,
        foul1,
        draw_move_count,
        last_info=None
    ))]
    fn new(
        sfen: &str,
        game_kind: u8,
        is_tsuitate: bool,
        promotion_rank: u8,
        foul0: i8,
        foul1: i8,
        draw_move_count: u16,
        last_info: Option<u8>,
    ) -> PyResult<Self> {
        let inner = GameApi::new(
            sfen,
            game_kind,
            is_tsuitate,
            promotion_rank,
            foul0,
            foul1,
            draw_move_count,
            last_info,
        )
        .map_err(|err| PyValueError::new_err(err.to_string()))?;
        Ok(Self { inner })
    }

    fn make_move(&mut self, csa_move: &str) -> bool {
        self.inner.make_move(csa_move)
    }

    fn make_move_ignore_turn(&mut self, csa_move: &str) -> bool {
        self.inner.make_move_ignore_turn(csa_move)
    }

    #[getter]
    fn sfen(&self) -> String {
        self.inner.sfen(None, false)
    }

    #[pyo3(signature = (csa_color=None, is_dark_shogi=false))]
    fn sfen_for(&self, csa_color: Option<&str>, is_dark_shogi: bool) -> PyResult<String> {
        Ok(self
            .inner
            .sfen(parse_optional_csa_color(csa_color)?, is_dark_shogi))
    }

    #[getter]
    fn fouls(&self) -> Vec<i8> {
        self.inner.fouls()
    }

    fn set_fouls(&mut self, foul0: i8, foul1: i8) {
        self.inner.set_fouls(foul0, foul1);
    }

    #[getter]
    fn last_info(&self) -> Option<u8> {
        self.inner.last_info().map(|info| info as u8)
    }

    #[getter]
    fn last_move(&self) -> Option<String> {
        self.inner.last_move_csa(None)
    }

    #[pyo3(signature = (csa_color=None))]
    fn last_move_for(&self, csa_color: Option<&str>) -> PyResult<Option<String>> {
        Ok(self
            .inner
            .last_move_csa(parse_optional_csa_color(csa_color)?))
    }

    #[getter]
    fn last_capture(&self) -> Option<String> {
        self.inner.last_capture()
    }

    fn is_valid(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        self.inner.is_valid(csa_move, is_tsuitate)
    }

    fn is_valid_ignore_turn(&self, csa_move: &str, is_tsuitate: bool) -> bool {
        self.inner.is_valid_ignore_turn(csa_move, is_tsuitate)
    }

    fn king_position(&self, csa_color: &str) -> Option<(u8, u8)> {
        csa_color_to_color(csa_color).and_then(|color| self.inner.king_position(color))
    }

    #[pyo3(signature = (csa_color, consecutive_fouls=None, last_lost_piece_square=None))]
    fn legal_action_indices(
        &self,
        csa_color: &str,
        consecutive_fouls: Option<Vec<(String, Option<bool>)>>,
        last_lost_piece_square: Option<(u8, u8)>,
    ) -> PyResult<Vec<usize>> {
        let color =
            csa_color_to_color(csa_color).ok_or_else(|| PyValueError::new_err("invalid color"))?;
        let mut position = self.inner.position().clone();
        position.side_to_move_set(color);
        let fouls = consecutive_fouls
            .unwrap_or_default()
            .into_iter()
            .map(|(action, in_check_before)| {
                let action = tsuitate_game::csa_to_move(&action, &position)
                    .map_err(|e| PyValueError::new_err(format!("invalid foul move: {e:?}")))?;
                Ok(crate::FoulAttempt {
                    action,
                    in_check_before,
                })
            })
            .collect::<PyResult<Vec<_>>>()?;
        let square = last_lost_piece_square
            .map(|(file, rank)| {
                shogi_core::Square::new(file, rank)
                    .ok_or_else(|| PyValueError::new_err("invalid capture square"))
            })
            .transpose()?;
        Ok(self.inner.legal_action_indices(
            color,
            Some(&crate::ActionHistory {
                consecutive_fouls: &fouls,
                last_lost_piece_square: square,
            }),
        ))
    }

    fn action_index_to_move(&self, action_index: usize) -> Option<String> {
        self.inner.action_index_to_csa_move(action_index)
    }

    fn move_action_indices_to(&self, file: u8, rank: u8) -> Vec<usize> {
        self.inner.move_action_indices_to(file, rank)
    }

    #[pyo3(name = "move_action_indices_to_square")]
    fn py_move_action_indices_to_square(&self, file: u8, rank: u8) -> Vec<usize> {
        self.inner.move_action_indices_to(file, rank)
    }

    fn from_candidates(
        &self,
        csa_color: &str,
        csa_from: &str,
        csa_piece: &str,
        is_tsuitate: bool,
        ignore_turn: bool,
    ) -> Vec<String> {
        let Some(color) = csa_color_to_color(csa_color) else {
            return Vec::new();
        };
        let csa_color = color_to_csa_sign(color);
        self.inner
            .from_candidates(csa_color, csa_from, csa_piece, is_tsuitate, ignore_turn)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGame>()?;
    m.add("INFO_NONE", Info::None as u8)?;
    m.add("INFO_FOUL", Info::Foul as u8)?;
    m.add("INFO_FOUL_UNDER_CHECK", Info::FoulUnderCheck as u8)?;
    m.add("INFO_CHECK", Info::Check as u8)?;
    m.add("INFO_CHECKMATE", Info::Checkmate as u8)?;
    m.add("INFO_LOSS_BY_FOUL", Info::LossByFoul as u8)?;
    m.add("INFO_DRAW", Info::Draw as u8)?;
    Ok(())
}
