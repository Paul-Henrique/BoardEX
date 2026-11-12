//! Formato BVR3 (BVRAW_FORMAT_3): linhas chave/valor e contorno segmentado.

use crate::formats::text::{Tokens, decode_text, find_str, split_lines};
use crate::model::{Board, FileFormat, Part, PartKind, Pin, Pt, Side};

pub fn detect(bytes: &[u8]) -> bool {
    find_str(bytes, "BVRAW_FORMAT_3")
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let text = decode_text(bytes);
    let mut board = Board::new(name, FileFormat::Bvr3);
    let mut part = Part::new(String::new());
    part.side = Side::Both;
    part.kind = PartKind::Smd;
    let mut pin = Pin::new(Pt::ZERO);
    let mut raw_segments: Vec<(Pt, Pt)> = Vec::new();
    let mut saw_any = false;

    for raw_line in split_lines(&text) {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split_at = line.find(char::is_whitespace).unwrap_or(line.len());
        let key = &line[..split_at];
        let rest = line[split_at..].trim();
        let mut t = Tokens::new(rest);

        match key {
            "PART_NAME" => {
                part.name = rest.to_string();
                saw_any = true;
            }
            "PART_SIDE" => {
                part.side = side_from_str(rest);
                saw_any = true;
            }
            "PART_ORIGIN" => {
                let x = t.next_f32().trunc();
                let y = t.next_f32().trunc();
                part.p1 = Pt::new(x, y);
                part.p2 = part.p1;
                saw_any = true;
            }
            "PART_MOUNT" => {
                let upper = rest.to_ascii_uppercase();
                part.kind = if upper.contains("TH") || upper.contains("THROUGH") {
                    PartKind::ThroughHole
                } else {
                    PartKind::Smd
                };
            }
            "PIN_NUMBER" => {
                pin.number = rest.to_string();
                saw_any = true;
            }
            "PIN_NAME" => {
                pin.name = rest.to_string();
            }
            "PIN_SIDE" => {
                pin.side = side_from_str(rest);
            }
            "PIN_ORIGIN" => {
                let x = t.next_f32().trunc();
                let y = t.next_f32().trunc();
                pin.pos = Pt::new(part.p1.x + x, part.p1.y + y);
            }
            "PIN_RADIUS" => {
                pin.radius = t.next_f32().max(0.0);
            }
            "PIN_NET" => {
                pin.net = rest.to_string();
            }
            "PIN_END" => {
                if pin.side == Side::Both && part.side != Side::Both {
                    pin.side = part.side;
                }
                pin.part = Some(board.parts.len());
                board
                    .pins
                    .push(std::mem::replace(&mut pin, Pin::new(Pt::ZERO)));
                saw_any = true;
            }
            "PART_END" => {
                part.pins.clear();
                board
                    .parts
                    .push(std::mem::replace(&mut part, Part::new(String::new())));
                saw_any = true;
            }
            "OUTLINE_POINTS" => {
                while !t.is_empty() {
                    let x = t.next_f32().trunc();
                    let y = t.next_f32().trunc();
                    board.outline.push(Pt::new(x, y));
                }
                saw_any = true;
            }
            "OUTLINE_SEGMENTED" => {
                while !t.is_empty() {
                    let x1 = t.next_f32().trunc();
                    let y1 = t.next_f32().trunc();
                    let x2 = t.next_f32().trunc();
                    let y2 = t.next_f32().trunc();
                    let a = Pt::new(x1, y1);
                    let b = Pt::new(x2, y2);
                    if a != b || !t.is_empty() {
                        raw_segments.push((a, b));
                    }
                }
                saw_any = true;
            }
            _ => {}
        }
    }

    if !saw_any {
        return Err("BVR3 sem dados reconhecidos".into());
    }

    board.segments = raw_segments.clone();
    if board.outline.is_empty() && !raw_segments.is_empty() {
        board.outline = assemble_segments(raw_segments);
    }

    board.finalize();
    Ok(board)
}

fn side_from_str(s: &str) -> Side {
    let s = s.trim();
    if s.eq_ignore_ascii_case("T") || s.eq_ignore_ascii_case("TOP") {
        Side::Top
    } else if s.eq_ignore_ascii_case("B") || s.eq_ignore_ascii_case("BOTTOM") {
        Side::Bottom
    } else {
        Side::Both
    }
}

/// Encadeia segmentos em um polígono, escolhendo o endpoint mais próximo quando
/// o encadeamento exato falha. Tolerante a pequenos erros de arredondamento.
pub(crate) fn assemble_segments(mut segments: Vec<(Pt, Pt)>) -> Vec<Pt> {
    let mut out = Vec::new();
    let Some((mut current, mut end)) = segments.pop() else {
        return out;
    };
    let start = current;
    out.push(current);
    out.push(end);

    let mut guard = 0usize;
    while !segments.is_empty() && guard < 1_000_000 {
        guard += 1;
        let exact = segments
            .iter()
            .position(|&(a, b)| a.distance(end) < 0.01 || b.distance(end) < 0.01);
        let (idx, next) = if let Some(idx) = exact {
            let (a, b) = segments[idx];
            let next = if a.distance(end) < 0.01 { b } else { a };
            (idx, next)
        } else {
            let mut best = 0usize;
            let mut best_d = f32::INFINITY;
            let mut best_next = end;
            let mut best_other = end;
            for (i, &(a, b)) in segments.iter().enumerate() {
                let da = a.distance(end);
                let db = b.distance(end);
                if da < best_d {
                    best_d = da;
                    best = i;
                    best_next = a;
                    best_other = b;
                }
                if db < best_d {
                    best_d = db;
                    best = i;
                    best_next = b;
                    best_other = a;
                }
            }
            (
                best,
                if best_next.distance(end) < 0.01 {
                    best_other
                } else {
                    best_next
                },
            )
        };

        segments.swap_remove(idx);
        if next.distance(start) < 0.01 {
            out.push(start);
            break;
        }
        if next.distance(end) > 0.01 {
            out.push(next);
        }
        current = next;
        end = next;
        let _ = current;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_bvr3() {
        let sample = "\
BVRAW_FORMAT_3
OUTLINE_POINTS 0 0 1000 0 1000 800 0 800
PART_NAME U1
PART_SIDE T
PART_ORIGIN 100 100
PART_MOUNT SMD
PIN_NUMBER 1
PIN_NAME VCC
PIN_SIDE T
PIN_ORIGIN 10 10
PIN_RADIUS 4
PIN_NET VCC
PIN_END
PART_END
";
        let board = parse(sample.as_bytes(), "t").unwrap();
        assert_eq!(board.parts.len(), 1);
        assert_eq!(board.pins.len(), 1);
        assert_eq!(board.outline.len(), 4);
        assert_eq!(board.pins[0].pos, Pt::new(110.0, 110.0));
        assert_eq!(board.pins[0].part, Some(0));
    }

    #[test]
    fn assemble_closed_square() {
        let pts = assemble_segments(vec![
            (Pt::new(0.0, 0.0), Pt::new(10.0, 0.0)),
            (Pt::new(10.0, 10.0), Pt::new(0.0, 10.0)),
            (Pt::new(10.0, 0.0), Pt::new(10.0, 10.0)),
            (Pt::new(0.0, 10.0), Pt::new(0.0, 0.0)),
        ]);
        assert!(pts.len() >= 5);
        assert_eq!(pts.first(), Some(&Pt::new(0.0, 10.0)));
        assert_eq!(pts.last(), Some(&Pt::new(0.0, 10.0)));
    }
}
