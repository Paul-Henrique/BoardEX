//! Formato BDV: texto com ofuscação por chave rolante.

use crate::formats::text::{Tokens, decode_text, find_str, split_lines};
use crate::model::{Board, FileFormat, Nail, Part, Pin, Pt, Side};

pub fn detect(bytes: &[u8]) -> bool {
    find_str(bytes, "dd:1.3?,r?-=bb")
        || (find_str(bytes, "<<format.asc>>") && find_str(bytes, "<<pins.asc>>"))
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let decoded = decode_bdv(bytes);
    let text = decode_text(&decoded);
    let lines = split_lines(&text);

    let mut board = Board::new(name, FileFormat::Bdv);
    let mut section = 0u8;
    let mut skip = 0usize;
    let mut saw_header = false;

    for raw_line in lines {
        let line = raw_line.trim();
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if line == "<<format.asc>>" {
            section = 1;
            skip = 8;
            saw_header = true;
            continue;
        }
        if line == "<<pins.asc>>" {
            section = 2;
            skip = 8;
            continue;
        }
        if line == "<<nails.asc>>" {
            section = 3;
            skip = 7;
            continue;
        }
        if line.starts_with("<<") {
            continue;
        }

        let mut t = Tokens::new(line);
        match section {
            1 => {
                let x = t.next_f32() * 1000.0;
                let y = t.next_f32() * 1000.0;
                board.outline.push(Pt::new(x, y));
            }
            2 => {
                if let Some(rest) = line.strip_prefix("Part") {
                    let mut pt = Tokens::new(rest);
                    let part_name = pt.next_str().to_string();
                    if part_name.is_empty() {
                        continue;
                    }
                    let location = pt.next_str();
                    let mut part = Part::new(part_name);
                    part.side = side_from_location(location);
                    board.parts.push(part);
                } else if !board.parts.is_empty() {
                    let number = t.next_str().to_string();
                    let pin_name = t.next_str().to_string();
                    let x = t.next_f32() * 1000.0;
                    let y = t.next_f32() * 1000.0;
                    let _layer = t.next_f32();
                    let net = t.next_str().to_string();
                    let probe = t.next_i32();
                    let part_index = board.parts.len() - 1;
                    let side = board.parts[part_index].side;
                    let mut pin = Pin::new(Pt::new(x, y));
                    pin.part = Some(part_index);
                    pin.number = number;
                    pin.name = pin_name;
                    pin.net = net;
                    pin.side = side;
                    pin.probe = probe;
                    board.pins.push(pin);
                }
            }
            3 => {
                // O OpenBoardView ignora exatamente o primeiro caractere do registro.
                let rest = line.get(1..).unwrap_or("");
                let mut t = Tokens::new(rest);
                let probe = t.next_i32();
                let x = t.next_f32() * 1000.0;
                let y = t.next_f32() * 1000.0;
                let _kind = t.next_i32();
                let _grid = t.next_str();
                let location = t.next_str();
                let _net_id = t.next_str();
                let net = t.next_str().to_string();
                let side = side_from_location(location);
                board.nails.push(Nail {
                    pos: Pt::new(x, y),
                    side,
                    net,
                    probe,
                });
            }
            _ => {}
        }
    }

    if !saw_header {
        return Err("BDV: seções não encontradas após decodificação".into());
    }

    board.finalize();
    Ok(board)
}

fn side_from_location(location: &str) -> Side {
    if location.contains('T') || location.contains('t') {
        Side::Top
    } else if location.contains('B') || location.contains('b') {
        Side::Bottom
    } else {
        Side::Both
    }
}

pub fn decode_bdv(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut count: i32 = 0xA0;
    for i in 0..out.len() {
        if out[i] == b'\r' && out.get(i + 1) == Some(&b'\n') {
            count += 1;
        }
        let x = out[i];
        if x != b'\r' && x != b'\n' && x != 0 {
            out[i] = (count - x as i32) as u8;
        }
        if count > 285 {
            count = 159;
        }
    }
    out
}

/// Codifica um texto no esquema BDV (usado apenas em testes).
#[cfg(test)]
pub fn encode_bdv(plain: &[u8]) -> Vec<u8> {
    let mut out = plain.to_vec();
    let mut count: i32 = 0xA0;
    for i in 0..out.len() {
        if out[i] == b'\r' && out.get(i + 1) == Some(&b'\n') {
            count += 1;
        }
        let x = out[i];
        if x != b'\r' && x != b'\n' && x != 0 {
            out[i] = (count - x as i32) as u8;
        }
        if count > 285 {
            count = 159;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_roundtrip() {
        let plain = b"abc\r\ndef\nghi";
        let enc = encode_bdv(plain);
        assert_eq!(decode_bdv(&enc), plain);
    }
}
