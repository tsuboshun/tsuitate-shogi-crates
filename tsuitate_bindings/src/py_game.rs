use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use shogi_core::{Color, PieceKind};
use tsuitate_game::csa_to_piece_kind;

use crate::game_api::{
    GameApi, INFO_CHECK, INFO_CHECKMATE, INFO_DRAW, INFO_FOUL, INFO_FOUL_UNDER_CHECK,
    INFO_LOSS_BY_FOUL, INFO_NONE,
};

fn parse_csa_color(csa_color: &str) -> Option<Color> {
    let mut chars = csa_color.chars();
    let color = match chars.next()? {
        '+' => Color::Black,
        '-' => Color::White,
        _ => return None,
    };
    if chars.next().is_some() {
        return None;
    }
    Some(color)
}

fn parse_optional_csa_color(csa_color: Option<&str>) -> PyResult<Option<Color>> {
    match csa_color {
        Some(value) => parse_csa_color(value)
            .map(Some)
            .ok_or_else(|| PyValueError::new_err("invalid color")),
        None => Ok(None),
    }
}

fn parse_excluded_piece_types(values: Option<Vec<String>>) -> PyResult<Vec<PieceKind>> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            csa_to_piece_kind(&value)
                .map_err(|_| PyValueError::new_err(format!("invalid piece type: {value}")))
        })
        .collect()
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
        self.inner.last_info()
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
        parse_csa_color(csa_color).and_then(|color| self.inner.king_position(color))
    }

    fn legal_action_indices(&self, csa_color: &str) -> Vec<usize> {
        parse_csa_color(csa_color)
            .map(|color| self.inner.legal_action_indices(color))
            .unwrap_or_default()
    }

    fn action_index_to_move(&self, action_index: usize) -> Option<String> {
        self.inner.action_index_to_csa_move(action_index)
    }

    fn move_action_indices_to(&self, file: u8, rank: u8) -> Vec<usize> {
        self.inner.move_action_indices_to(file, rank)
    }

    #[pyo3(signature = (
        csa_color,
        exclude_piece_types=None,
        treat_friendly_target_as_empty=true
    ))]
    /// Count attacks by `csa_color` in SFEN board order (rank 1 first and
    /// descending files within each rank).
    ///
    /// If `treat_friendly_target_as_empty` is true, friendly occupied squares
    /// are counted as defended. Such pieces still block sliding attacks beyond
    /// their squares. If it is false, every friendly occupied square has an
    /// attack count of zero and still blocks sliding attacks beyond it.
    fn attack_counts(
        &self,
        csa_color: &str,
        exclude_piece_types: Option<Vec<String>>,
        treat_friendly_target_as_empty: bool,
    ) -> PyResult<Vec<u8>> {
        let color =
            parse_csa_color(csa_color).ok_or_else(|| PyValueError::new_err("invalid color"))?;
        let excluded_piece_types = parse_excluded_piece_types(exclude_piece_types)?;
        Ok(self
            .inner
            .attack_counts(color, &excluded_piece_types, treat_friendly_target_as_empty))
    }

    #[pyo3(signature = (
        csa_color,
        moves,
        include_attack_counts=true,
        exclude_piece_types=None,
        treat_friendly_target_as_empty=true
    ))]
    /// Apply every CSA move to an independent clone and return all results.
    /// Invalid moves report the unchanged clone's state. For included attack
    /// counts, `treat_friendly_target_as_empty` has the same meaning as in
    /// `attack_counts`: friendly occupied squares are counted as defended but
    /// still block sliding attacks beyond them when true; when false, their
    /// attack counts are zero. They block sliding attacks in either case.
    fn analyze_moves(
        &self,
        py: Python<'_>,
        csa_color: &str,
        moves: Vec<String>,
        include_attack_counts: bool,
        exclude_piece_types: Option<Vec<String>>,
        treat_friendly_target_as_empty: bool,
    ) -> PyResult<Vec<Py<PyDict>>> {
        let color =
            parse_csa_color(csa_color).ok_or_else(|| PyValueError::new_err("invalid color"))?;
        let excluded_piece_types = parse_excluded_piece_types(exclude_piece_types)?;
        self.inner
            .analyze_moves(
                &moves,
                color,
                include_attack_counts,
                &excluded_piece_types,
                treat_friendly_target_as_empty,
            )
            .into_iter()
            .map(|result| {
                let dict = PyDict::new(py);
                dict.set_item("move", result.csa_move)?;
                dict.set_item("valid", result.valid)?;
                dict.set_item("last_info", result.last_info)?;
                dict.set_item("last_capture", result.last_capture)?;
                dict.set_item("sfen", result.sfen)?;
                dict.set_item("fouls", result.fouls)?;
                dict.set_item("attack_counts", result.attack_counts)?;
                Ok(dict.unbind())
            })
            .collect()
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
        let Some(color) = parse_csa_color(csa_color) else {
            return Vec::new();
        };
        let csa_color = match color {
            Color::Black => '+',
            Color::White => '-',
        };
        self.inner
            .from_candidates(csa_color, csa_from, csa_piece, is_tsuitate, ignore_turn)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGame>()?;
    m.add("INFO_NONE", INFO_NONE)?;
    m.add("INFO_FOUL", INFO_FOUL)?;
    m.add("INFO_FOUL_UNDER_CHECK", INFO_FOUL_UNDER_CHECK)?;
    m.add("INFO_CHECK", INFO_CHECK)?;
    m.add("INFO_CHECKMATE", INFO_CHECKMATE)?;
    m.add("INFO_LOSS_BY_FOUL", INFO_LOSS_BY_FOUL)?;
    m.add("INFO_DRAW", INFO_DRAW)?;
    Ok(())
}
