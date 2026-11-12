//! Formato BRD2 ("BRDOUT:" / "NETS:" / "PARTS:" / "PINS:" / "NAILS:").

use std::collections::HashMap;

use crate::formats::text::{Tokens, decode_text, find_str, split_lines};
use crate::model::{BBox, Board, FileFormat, Nail, Part, PartKind, Pin, Pt, Side};

pub fn detect(bytes: &[u8]) -> bool {
    find_str(bytes, "BRDOUT:") && find_str(bytes, "NETS:")
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let text = decode_text(bytes);
    let lines = split_lines(&text);

    let mut board = Board::new(name, FileFormat::Brd2);
    let mut section = 0u8;
    let mut nets: HashMap<i32, String> = HashMap::new();
    let mut max = Pt::ZERO;
    let mut expected_nets = 0usize;
    let mut expected_parts = 0usize;
    let mut expected_pins = 0usize;
    let mut expected_nails = 0usize;
    let mut part_starts: Vec<usize> = Vec::new();
    let mut saw_header = false;

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("BRDOUT:") {
            section = 1;
            saw_header = true;
            let mut t = Tokens::new(rest);
            let _format = t.next_i32();
            max = Pt::new(t.next_f32(), t.next_f32());
            continue;
        }
        if let Some(rest) = line.strip_prefix("NETS:") {
            section = 2;
            expected_nets = Tokens::new(rest).next_u32() as usize;
            continue;
        }
        if let Some(rest) = line.strip_prefix("PARTS:") {
            section = 3;
            expected_parts = Tokens::new(rest).next_u32() as usize;
            continue;
        }
        if let Some(rest) = line.strip_prefix("PINS:") {
            section = 4;
            expected_pins = Tokens::new(rest).next_u32() as usize;
            continue;
        }
        if let Some(rest) = line.strip_prefix("NAILS:") {
            section = 5;
            expected_nails = Tokens::new(rest).next_u32() as usize;
            continue;
        }

        let mut t = Tokens::new(line);
        match section {
            2 => {
                let id = t.next_i32();
                let net_name = t.next_str().to_string();
                if !net_name.is_empty() {
                    nets.insert(id, net_name);
                }
            }
            3 => {
                let part_name = t.next_str().to_string();
                if part_name.is_empty() {
                    continue;
                }
                let p1 = Pt::new(t.next_f32(), t.next_f32());
                let p2 = Pt::new(t.next_f32(), t.next_f32());
                let start_pin = t.next_u32() as usize;
                let side_num = t.next_u32();
                let side = match side_num {
                    1 => Side::Top,
                    2 => Side::Bottom,
                    _ => Side::Both,
                };
                let mut part = Part::new(part_name);
                part.p1 = p1;
                part.p2 = p2;
                part.side = side;
                part.kind = PartKind::Smd;
                part_starts.push(start_pin);
                board.parts.push(part);
            }
            4 => {
                let x = t.next_f32();
                let y = t.next_f32();
                let net_id = t.next_i32();
                let side_num = t.next_u32();
                let side = match side_num {
                    1 => Side::Top,
                    2 => Side::Bottom,
                    _ => Side::Both,
                };
                let mut pin = Pin::new(Pt::new(x, y));
                pin.side = side;
                pin.net = nets.get(&net_id).cloned().unwrap_or_default();
                board.pins.push(pin);
            }
            5 => {
                let probe = t.next_i32();
                let x = t.next_f32();
                let y = t.next_f32();
                let net_id = t.next_i32();
                let side_num = t.next_u32();
                let side = match side_num {
                    1 => Side::Top,
                    2 => Side::Bottom,
                    _ => Side::Both,
                };
                let net = nets.get(&net_id).cloned().unwrap_or_default();
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
        return Err("cabeçalho BRD2 não encontrado".into());
    }

    // Componentes do lado inferior e pinos fora do lado superior são espelhados em Y.
    for (i, part) in board.parts.iter_mut().enumerate() {
        if part.side == Side::Bottom {
            part.p1.y = max.y - part.p1.y;
            part.p2.y = max.y - part.p2.y;
        }
        let start = part_starts.get(i).copied().unwrap_or(0);
        let _ = start;
    }

    let mut cursor = 0usize;
    for i in 0..board.parts.len() {
        let end = part_starts
            .get(i + 1)
            .copied()
            .unwrap_or(board.pins.len())
            .min(board.pins.len());
        let start = cursor;
        let mut is_dip = true;
        while cursor < end {
            board.pins[cursor].part = Some(i);
            if board.pins[cursor].side != Side::Top {
                board.pins[cursor].pos.y = max.y - board.pins[cursor].pos.y;
            }
            if (board.pins[cursor].side == Side::Top && board.parts[i].side == Side::Top)
                || (board.pins[cursor].side == Side::Bottom && board.parts[i].side == Side::Bottom)
            {
                is_dip = false;
            }
            cursor += 1;
        }
        if is_dip && cursor > start {
            board.parts[i].kind = PartKind::ThroughHole;
            board.parts[i].side = Side::Both;
        } else {
            board.parts[i].kind = PartKind::Smd;
        }
    }

    if expected_nets != 0 && expected_nets != nets.len() {
        board.warnings.push(format!(
            "BRD2: {} nets esperadas, {} lidas",
            expected_nets,
            nets.len()
        ));
    }
    if expected_parts != 0 && expected_parts != board.parts.len() {
        board.warnings.push(format!(
            "BRD2: {} componentes esperados, {} lidos",
            expected_parts,
            board.parts.len()
        ));
    }
    if expected_pins != 0 && expected_pins != board.pins.len() {
        board.warnings.push(format!(
            "BRD2: {} pinos esperados, {} lidos",
            expected_pins,
            board.pins.len()
        ));
    }
    if expected_nails != 0 && expected_nails != board.nails.len() {
        board.warnings.push(format!(
            "BRD2: {} nails esperados, {} lidos",
            expected_nails,
            board.nails.len()
        ));
    }

    // Nails do lado inferior também são armazenados na coordenada da vista inferior.
    if max.y > 0.0 {
        for nail in &mut board.nails {
            if nail.side == Side::Bottom {
                nail.pos.y = max.y - nail.pos.y;
            }
        }
    }

    if max.x > 0.0 && max.y > 0.0 {
        board.limits = Some(BBox::from_two(Pt::ZERO, max));
    }

    board.finalize();
    Ok(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_brd2() {
        let sample = "\
BRDOUT: 2 1000 800
NETS: 2
1 GND
2 VCC
PARTS: 1
U1 100 100 300 300 0 1
PINS: 2
100 100 1 1
200 100 2 1
NAILS: 2
9 500 500 1 1
10 500 200 1 2
";
        let board = parse(sample.as_bytes(), "t").unwrap();
        assert_eq!(board.parts.len(), 1);
        assert_eq!(board.pins.len(), 2);
        assert_eq!(board.nails.len(), 2);
        assert_eq!(board.pins[0].net, "GND");
        assert_eq!(board.pins[1].net, "VCC");
        assert_eq!(board.pins[0].part, Some(0));
        assert_eq!(board.parts[0].pins.len(), 2);
        assert!(!board.parts[0].bounds.contains(Pt::new(500.0, 500.0)));
        assert_eq!(board.nails[1].side, Side::Bottom);
        assert_eq!(board.nails[1].pos, Pt::new(500.0, 600.0));
        assert!(board.nets.iter().any(|net| net == "GND"));
        assert!(board.bounds.width() >= 400.0);
    }
}
