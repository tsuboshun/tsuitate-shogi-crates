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


def test_attack_counts_supports_friendly_targets():
    game = tb.Game("sfen 9/9/9/9/9/4P4/9/4R4/9 b - 1", 1, False, 3, 9, 9, 150)
    index = lambda file, rank: (rank - 1) * 9 + (9 - file)

    counts = game.attack_counts("+", treat_friendly_target_as_empty=True)
    assert len(counts) == 81
    assert counts[index(5, 7)] == 1
    assert counts[index(5, 6)] == 1

    assert game.attack_counts(
        "+", treat_friendly_target_as_empty=False
    )[index(5, 6)] == 0

    with pytest.raises(ValueError):
        game.attack_counts("*")


@pytest.mark.parametrize(
    ("piece", "near", "far"),
    [
        ("R", (5, 4), (5, 3)),
        ("B", (4, 4), (3, 3)),
        ("+R", (5, 4), (5, 3)),
        ("+B", (4, 4), (3, 3)),
        ("L", (5, 4), (5, 3)),
    ],
)
def test_attack_counts_can_limit_sliding_piece_distance(piece, near, far):
    game = tb.Game(
        f"sfen 9/9/9/9/4{piece}4/9/9/9/9 b - 1", 1, False, 3, 9, 9, 150
    )
    index = lambda file, rank: (rank - 1) * 9 + (9 - file)

    unlimited = game.attack_counts("+")
    limited = game.attack_counts("+", max_sliding_distance=1)
    zero = game.attack_counts("+", max_sliding_distance=0)

    assert unlimited[index(*far)] == 1
    assert limited[index(*near)] == 1
    assert limited[index(*far)] == 0
    assert zero[index(*near)] == 0


def test_attack_counts_distance_limit_does_not_affect_short_range_pieces():
    game = tb.Game("sfen 9/9/9/9/4P4/9/9/9/9 b - 1", 1, False, 3, 9, 9, 150)
    index = lambda file, rank: (rank - 1) * 9 + (9 - file)

    assert game.attack_counts("+", max_sliding_distance=0)[index(5, 4)] == 1


def test_analyze_moves_returns_batch_results_without_mutating_game():
    game = new_standard_game()
    original_sfen = game.sfen

    results = game.analyze_moves(
        "+", ["+7776FU", "+7775FU"], include_attack_counts=True
    )

    assert [result["move"] for result in results] == ["+7776FU", "+7775FU"]
    assert results[0]["valid"] is True
    assert results[0]["last_info"] == tb.INFO_NONE
    assert results[0]["last_capture"] is None
    assert results[0]["fouls"] == (9, 9)
    assert len(results[0]["attack_counts"]) == 81
    assert results[1]["valid"] is False
    assert results[1]["sfen"] == original_sfen
    assert game.sfen == original_sfen
    assert game.last_move is None


def test_analyze_moves_can_skip_attack_counts():
    result = new_standard_game().analyze_moves(
        "+", ["+7776FU"], include_attack_counts=False
    )[0]

    assert result["attack_counts"] is None


def test_analyze_moves_can_limit_sliding_piece_distance():
    game = tb.Game("sfen 9/9/9/9/4R4/9/9/9/9 b - 1", 1, False, 3, 9, 9, 150)
    index = lambda file, rank: (rank - 1) * 9 + (9 - file)

    result = game.analyze_moves(
        "+", ["+5554HI"], max_sliding_distance=1
    )[0]

    assert result["valid"] is True
    assert result["attack_counts"][index(5, 3)] == 1
    assert result["attack_counts"][index(5, 2)] == 0


def test_analyze_moves_reports_captured_piece():
    game = tb.Game("sfen 9/9/9/9/9/9/4p4/4R4/9 b - 1", 1, False, 3, 9, 9, 150)

    result = game.analyze_moves("+", ["+5857HI"], include_attack_counts=False)[0]

    assert result["valid"] is True
    assert result["last_capture"] == "P"
