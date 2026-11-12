//! Formato BVR (BVRAW_FORMAT_1): seções com tabulações, coordenadas em polegadas.

use std::collections::HashMap;

use crate::formats::text::{Tokens, decode_text, find_str, parse_f32, split_lines};
use crate::model::{Board, FileFormat, Nail, Part, Pin, Pt, Side};

pub fn detect(bytes: &[u8]) -> bool {
    find_str(bytes, "BVRAW_FORMAT_1")
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let text = decode_text(bytes);
    let lines = split_lines(&text);

    let mut board = Board::new(name, FileFormat::Bvr);
    let mut section = 0u8;
    let mut part_map: HashMap<String, usize> = HashMap::new();
    let mut saw_layout = false;

    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() {
            continue;
        }
        match line {
            "<<Layout>>" => {
                section = 1;
                saw_layout = true;
                i += 1; // cabeçalho é seguido por uma linha de metadados
                continue;
            }
            "<<Pin>>" => {
                section = 2;
                i += 1;
                continue;
            }
            "<<Nail>>" => {
                section = 3;
                i += 1;
                continue;
            }
            _ => {}
        }

        match section {
            1 => {
                // No BVR, x e y podem vir separados apenas por vírgula.
                let normalized = line.replace(',', " ");
                let mut t = Tokens::new(&normalized);
                let x = (t.next_f32() * 1000.0).trunc();
                let y = (t.next_f32() * 1000.0).trunc();
                board.outline.push(Pt::new(x, y));
            }
            2 => {
                let fields = tab_fields(line);
                let part_name = fields.first().copied().unwrap_or("").to_string();
                if part_name.is_empty() {
                    continue;
                }
                let location = fields.get(1).copied().unwrap_or("");
                let number = fields
                    .get(2)
                    .copied()
                    .unwrap_or("")
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .to_string();
                let pin_name = fields.get(3).copied().unwrap_or("").to_string();
                let x = (parse_f32(fields.get(4).copied().unwrap_or("0")) * 1000.0).trunc();
                let y = (parse_f32(fields.get(5).copied().unwrap_or("0")) * 1000.0).trunc();
                let net = fields.get(7).copied().unwrap_or("").to_string();

                let side = side_from_location(location);
                let part_index = if let Some(&idx) = part_map.get(&part_name) {
                    idx
                } else {
                    let mut part = Part::new(part_name.clone());
                    part.side = side;
                    board.parts.push(part);
                    let idx = board.parts.len() - 1;
                    part_map.insert(part_name, idx);
                    idx
                };
                if board.parts[part_index].side == Side::Both {
                    board.parts[part_index].side = side;
                } else if board.parts[part_index].side != side {
                    board.parts[part_index].side = Side::Both;
                }

                let mut pin = Pin::new(Pt::new(x, y));
                pin.part = Some(part_index);
                pin.number = number;
                pin.name = pin_name;
                pin.net = net;
                pin.side = side;
                board.pins.push(pin);
            }
            3 => {
                // Campos separados por tabulação; o primeiro campo é ignorado,
                // como em nextfield() no formato original.
                let fields = tab_fields(line);
                if fields.len() < 8 {
                    continue;
                }
                let x = (parse_f32(fields[1]) * 1000.0).trunc();
                let y = (parse_f32(fields[2]) * 1000.0).trunc();
                let location = fields[5];
                let net = fields[7].to_string();
                let side = side_from_location(location);
                board.nails.push(Nail {
                    pos: Pt::new(x, y),
                    side,
                    net,
                    probe: 0,
                });
            }
            _ => {}
        }
    }

    if !saw_layout && board.parts.is_empty() {
        return Err("BVR sem seções reconhecidas".into());
    }

    board.finalize();
    Ok(board)
}

fn tab_fields(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        line.split('\t').map(str::trim).collect()
    } else {
        line.split_whitespace().collect()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_bvr() {
        let sample = "\
BVRAW_FORMAT_1
<<Layout>>
layout-header
0.0,0.0
1.0,0.0
1.0,0.8
0.0,0.8
<<Pin>>
header
U1\t(T)\t1\tVCC\t0.100\t0.100\t1\tVCC
U1\t(T)\t2\tGND\t0.200\t0.100\t1\tGND
<<Nail>>
header
nail\t0.5\t0.5\t0\t\t(T)\t1\tGND
";
        let board = parse(sample.as_bytes(), "t").unwrap();
        assert_eq!(board.parts.len(), 1);
        assert_eq!(board.pins.len(), 2);
        assert_eq!(board.nails.len(), 1);
        assert_eq!(board.outline.len(), 4);
        assert_eq!(board.outline[1], Pt::new(1000.0, 0.0));
        assert_eq!(board.outline[2], Pt::new(1000.0, 800.0));
        assert_eq!(board.pins[0].net, "VCC");
        assert_eq!(board.pins[0].side, Side::Top);
        assert_eq!(board.nails[0].pos, Pt::new(500.0, 500.0));
        assert_eq!(board.nails[0].side, Side::Top);
        assert_eq!(board.nails[0].net, "GND");
    }
}
