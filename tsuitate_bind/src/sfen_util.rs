fn split_sfen_parts(sfen: &str) -> (&str, &str, &str, &str, bool) {
    let parts: Vec<&str> = sfen.split_whitespace().collect();
    match parts.as_slice() {
        ["sfen", board, turn, hands, move_number, ..] => {
            (*board, *turn, *hands, *move_number, true)
        }
        [board, turn, hands, move_number, ..] => (*board, *turn, *hands, *move_number, false),
        _ => panic!("invalid sfen format"),
    }
}

fn expand_row(row: &str) -> Vec<String> {
    let chars: Vec<char> = row.chars().collect();
    let mut cells = Vec::with_capacity(9);
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let count = c.to_digit(10).unwrap() as usize;
            for _ in 0..count {
                cells.push(String::new());
            }
            i += 1;
            continue;
        }
        if c == '+' {
            if i + 1 >= chars.len() {
                panic!("invalid promotion in sfen");
            }
            cells.push(format!("{}{}", c, chars[i + 1]));
            i += 2;
            continue;
        }
        cells.push(c.to_string());
        i += 1;
    }
    cells
}

fn compress_row(cells: &[String]) -> String {
    let mut out = String::new();
    let mut empty = 0usize;
    for cell in cells {
        if cell.is_empty() {
            empty += 1;
        } else {
            if empty > 0 {
                out.push_str(&empty.to_string());
                empty = 0;
            }
            out.push_str(cell);
        }
    }
    if empty > 0 {
        out.push_str(&empty.to_string());
    }
    out
}

pub fn normalize_sfen_to_9x9(sfen: &str) -> (String, u8, u8) {
    let (board, turn, hands, move_number, _) = split_sfen_parts(sfen);
    let rows: Vec<&str> = board.split('/').collect();
    let ranks = rows.len();
    if !(1..=9).contains(&ranks) {
        panic!("invalid ranks");
    }

    let mut files: Option<usize> = None;
    let mut expanded_rows = Vec::with_capacity(9);
    for row in rows {
        let cells = expand_row(row);
        let count = cells.len();
        if !(1..=9).contains(&count) {
            panic!("invalid files");
        }
        if let Some(prev) = files {
            if prev != count {
                panic!("non-rectangular board");
            }
        } else {
            files = Some(count);
        }

        let left_pad = 9usize.saturating_sub(count);
        let mut padded = Vec::with_capacity(9);
        for _ in 0..left_pad {
            padded.push(String::new());
        }
        padded.extend(cells);
        expanded_rows.push(compress_row(&padded));
    }

    while expanded_rows.len() < 9 {
        expanded_rows.push("9".to_string());
    }

    let board_9x9 = expanded_rows.join("/");
    (
        format!("sfen {} {} {} {}", board_9x9, turn, hands, move_number),
        files.unwrap() as u8,
        ranks as u8,
    )
}

pub fn denormalize_sfen_from_9x9(sfen: &str, files: u8, ranks: u8) -> String {
    let (board, turn, hands, move_number, has_prefix) = split_sfen_parts(sfen);
    let rows: Vec<&str> = board.split('/').collect();
    if rows.len() != 9 {
        panic!("expected 9x9 sfen");
    }
    let files = files as usize;
    let ranks = ranks as usize;
    if !(1..=9).contains(&files) || !(1..=9).contains(&ranks) {
        panic!("invalid board size");
    }

    let mut trimmed_rows = Vec::with_capacity(ranks);
    for row in rows.iter().take(ranks) {
        let cells = expand_row(row);
        if cells.len() != 9 {
            panic!("invalid 9x9 row");
        }
        let tail = &cells[(9 - files)..];
        trimmed_rows.push(compress_row(tail));
    }

    let board_rect = trimmed_rows.join("/");
    if has_prefix {
        format!("sfen {} {} {} {}", board_rect, turn, hands, move_number)
    } else {
        format!("{} {} {} {}", board_rect, turn, hands, move_number)
    }
}
