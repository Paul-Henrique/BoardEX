//! Formato BRD (boardview texto, opcionalmente ofuscado com XOR/rotação).

use crate::formats::text::{Tokens, decode_text, find_str, split_lines};
use crate::model::{Board, FileFormat, Nail, Part, PartKind, Pin, Pt, Side};

const ENCODED_HEADER: [u8; 4] = [0x23, 0xE2, 0x63, 0x28];

pub fn detect(bytes: &[u8]) -> bool {
    bytes.starts_with(&ENCODED_HEADER)
        || (find_str(bytes, "str_length:") && find_str(bytes, "var_data:"))
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let decoded = decode_brd(bytes);
    let text = decode_text(&decoded);
    let lines = split_lines(&text);

    let mut board = Board::new(name, FileFormat::Brd);
    let mut section = 0u8;
    let mut counts = (0usize, 0usize, 0usize, 0usize);
    let mut saw_header = false;

    for raw_line in lines {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "str_length:" => {
                section = 1;
                saw_header = true;
                continue;
            }
            "var_data:" => {
                section = 2;
                saw_header = true;
                continue;
            }
            "Format:" | "format:" => {
                section = 3;
                continue;
            }
            "Parts:" | "Pins1:" => {
                section = 4;
                continue;
            }
            "Pins:" | "Pins2:" => {
                section = 5;
                continue;
            }
            "Nails:" => {
                section = 6;
                continue;
            }
            _ => {}
        }

        let mut t = Tokens::new(line);
        match section {
            2 => {
                counts.0 = t.next_u32() as usize;
                counts.1 = t.next_u32() as usize;
                counts.2 = t.next_u32() as usize;
                counts.3 = t.next_u32() as usize;
            }
            3 => {
                let x = t.next_i32() as f32;
                let y = t.next_i32() as f32;
                board.outline.push(Pt::new(x, y));
            }
            4 => {
                let part_name = t.next_str().to_string();
                if part_name.is_empty() {
                    continue;
                }
                let flags = t.next_u32();
                let end_of_pins = t.next_u32() as usize;
                let mut part = Part::new(part_name);
                part.kind = if flags & 0x0C != 0 {
                    PartKind::Smd
                } else {
                    PartKind::ThroughHole
                };
                part.side = if flags == 1 || (4..8).contains(&flags) {
                    Side::Top
                } else if flags == 2 || flags >= 8 {
                    Side::Bottom
                } else {
                    Side::Both
                };
                if end_of_pins <= counts.2 {
                    part.pins.reserve(0); // apenas validação leve
                }
                board.parts.push(part);
            }
            5 => {
                let x = t.next_i32() as f32;
                let y = t.next_i32() as f32;
                let probe = t.next_i32();
                let part_index = t.next_i32();
                let net = t.next_str().to_string();
                let part = if part_index >= 1 && (part_index as usize) <= board.parts.len() {
                    Some(part_index as usize - 1)
                } else {
                    None
                };
                let side = part.map(|i| board.parts[i].side).unwrap_or(Side::Both);
                let mut pin = Pin::new(Pt::new(x, y));
                pin.probe = probe;
                pin.part = part;
                pin.net = net;
                pin.side = side;
                board.pins.push(pin);
            }
            6 => {
                let probe = t.next_i32();
                let x = t.next_i32() as f32;
                let y = t.next_i32() as f32;
                let side = if t.next_u32() == 1 {
                    Side::Top
                } else {
                    Side::Bottom
                };
                let net = t.next_str().to_string();
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
        return Err("cabeçalho BRD não encontrado".into());
    }

    // Variante Lenovo: alguns pinos vêm sem net e o nome é obtido pelo probe/nail.
    let nail_nets: std::collections::HashMap<i32, String> = board
        .nails
        .iter()
        .map(|nail| (nail.probe, nail.net.clone()))
        .collect();
    for pin in &mut board.pins {
        if pin.net.is_empty()
            && let Some(net) = nail_nets.get(&pin.probe)
        {
            pin.net = net.clone();
        }
    }

    let actual = (
        board.outline.len(),
        board.parts.len(),
        board.pins.len(),
        board.nails.len(),
    );
    if counts.0 != 0 && counts.0 != actual.0 {
        board.warnings.push(format!(
            "BRD: {} pontos de contorno esperados, {} lidos",
            counts.0, actual.0
        ));
    }
    if counts.1 != 0 && counts.1 != actual.1 {
        board.warnings.push(format!(
            "BRD: {} componentes esperados, {} lidos",
            counts.1, actual.1
        ));
    }
    if counts.2 != 0 && counts.2 != actual.2 {
        board.warnings.push(format!(
            "BRD: {} pinos esperados, {} lidos",
            counts.2, actual.2
        ));
    }

    board.finalize();
    Ok(board)
}

fn decode_brd(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if !out.starts_with(&ENCODED_HEADER) {
        return out;
    }
    for b in &mut out {
        if *b == b'\r' || *b == b'\n' || *b == 0 {
            continue;
        }
        let c = *b as i8 as i32;
        let v = !(((c >> 6) & 3) | (c << 2));
        *b = v as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_brd() {
        let sample = "\
str_length:\n\
var_data:\n\
4 2 3 1\n\
Format:\n\
0 0\n\
1000 0\n\
1000 800\n\
0 800\n\
Parts:\n\
U1 5 4\n\
R1 1 4\n\
Pins:\n\
100 100 1 1 GND\n\
200 100 2 1 VCC\n\
300 300 3 2 N1\n\
Nails:\n\
1 500 500 1 GND\n";
        let board = parse(sample.as_bytes(), "test").unwrap();
        assert_eq!(board.outline.len(), 4);
        assert_eq!(board.parts.len(), 2);
        assert_eq!(board.pins.len(), 3);
        assert_eq!(board.nails.len(), 1);
        assert_eq!(board.parts[0].side, Side::Top);
        assert_eq!(board.parts[1].kind, PartKind::ThroughHole);
        assert_eq!(board.pins[0].part, Some(0));
        assert_eq!(board.pins[0].net, "GND");
        assert!(board.nets.contains(&"VCC".to_string()));
    }

    #[test]
    fn decode_encoded_header() {
        let plain = b"str_length:\nvar_data:\n4 2 3 1\nFormat:\n0 0\n1000 0\n1000 800\n0 800\nParts:\nU1 5 4\nR1 1 4\nPins:\n100 100 1 1 GND\n200 100 2 1 VCC\n300 300 3 2 N1\nNails:\n1 500 500 1 GND\n";
        let mut encoded: Vec<u8> = plain
            .iter()
            .map(|&b| {
                if b == b'\r' || b == b'\n' || b == 0 {
                    b
                } else {
                    let u = !b;
                    ((u & 3) << 6) | (u >> 2)
                }
            })
            .collect();
        encoded[..4].copy_from_slice(&ENCODED_HEADER);
        assert_eq!(decode_brd(&encoded), plain);
        let board = parse(&encoded, "test").unwrap();
        assert_eq!(board.parts.len(), 2);
        assert_eq!(board.pins.len(), 3);
    }

    #[test]
    fn pin_net_from_nail_probe() {
        let sample = "\
str_length:\n\
var_data:\n\
4 1 1 1\n\
Format:\n\
0 0\n\
1000 0\n\
1000 800\n\
0 800\n\
Parts:\n\
U1 5 1\n\
Pins:\n\
100 100 7 1\n\
Nails:\n\
7 200 200 1 GND\n";
        let board = parse(sample.as_bytes(), "test").unwrap();
        assert_eq!(board.pins.len(), 1);
        assert_eq!(board.pins[0].probe, 7);
        assert_eq!(board.pins[0].net, "GND");
        assert!(board.nets.contains(&"GND".to_string()));
    }
}
