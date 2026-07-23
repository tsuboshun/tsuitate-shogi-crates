import pytest

import tsuitate_bindings as tb


INITIAL_SFEN = (
    "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/"
    "PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
)


def new_standard_game():
    return tb.Game(INITIAL_SFEN, 1, False, 3, 9, 9, 150)


def test_make_move_updates_sfen():
    game = new_standard_game()

    assert game.last_move is None
    assert game.make_move("+7776FU")
    assert game.last_move == "+7776FU"
    assert (
        game.sfen
        == "lnsgkgsnl/1r5b1/ppppppppp/9/9/2P6/"
        "PP1PPPPPP/1B5R1/LNSGKGSNL w - 2"
    )


def test_viewpoint_sfen_and_last_move_helpers():
    game = new_standard_game()

    assert (
        game.sfen_for("+")
        == "9/9/9/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
    )
    assert "?" in game.sfen_for("+", True)
    assert game.make_move("+7776FU")
    assert game.last_move_for("+") == "+7776FU"
    assert game.last_move_for("-") == "+0000ZZ"


def test_make_move_ignore_turn_restores_turn_after_failure():
    game = new_standard_game()

    assert not game.make_move_ignore_turn("+7775FU")
    assert game.sfen == INITIAL_SFEN.removeprefix("sfen ")


def test_fouls_property_and_setter():
    game = new_standard_game()

    assert game.fouls == [9, 9]
    game.set_fouls(1, 2)
    assert game.fouls == [1, 2]


def test_constructor_accepts_initial_last_info():
    game = tb.Game(INITIAL_SFEN, 1, False, 3, 9, 9, 150, last_info=tb.INFO_CHECK)

    assert game.last_info == tb.INFO_CHECK


def test_from_candidates_returns_empty_for_invalid_input():
    game = new_standard_game()

    assert game.from_candidates("*", "77", "FU", False, False) == []
    assert game.from_candidates("+", "7", "FU", False, False) == []
    assert game.from_candidates("+", "7a", "FU", False, False) == []
    assert game.from_candidates("+", "00", "XX", False, False) == []
    assert game.from_candidates("++", "77", "FU", False, False) == []


def test_action_helpers_are_exposed():
    game = new_standard_game()
    black_pawn_push = ((7 - 1) * 9 + (6 - 1)) * 27
    white_pawn_push = ((3 - 1) * 9 + (4 - 1)) * 27

    assert game.king_position("+") == (5, 9)
    assert game.king_position("-") == (5, 1)
    assert game.king_position("*") is None

    assert black_pawn_push in game.legal_action_indices("+")
    assert white_pawn_push in game.legal_action_indices("-")
    assert game.legal_action_indices("*") == []

    assert game.action_index_to_move(black_pawn_push) == "+7776FU"
    assert game.action_index_to_move(9 * 9 * 27) is None

    assert game.move_action_indices_to(7, 6) == [black_pawn_push]
    assert game.move_action_indices_to_square(7, 6) == [black_pawn_push]
    assert game.move_action_indices_to(3, 4) == []
    assert game.move_action_indices_to(0, 6) == []

    assert game.make_move("+7776FU")
    assert game.move_action_indices_to(3, 4) == [white_pawn_push]


def test_invalid_constructor_input_raises_value_error():
    with pytest.raises(ValueError):
        tb.Game("invalid", 1, False, 3, 9, 9, 150)

    with pytest.raises(ValueError):
        tb.Game(INITIAL_SFEN, 0, False, 3, 9, 9, 150)

    with pytest.raises(ValueError):
        tb.Game(INITIAL_SFEN, 1, False, 3, 9, 9, 150, last_info=7)


def test_info_constants_are_exported():
    assert tb.INFO_NONE == 0
    assert tb.INFO_FOUL == 1
    assert tb.INFO_DRAW == 6
