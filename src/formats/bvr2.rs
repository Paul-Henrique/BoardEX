//! Formatos BVR2 e FBV.
//!
//! FBV é o corpo BVR2 cifrado com XOR: o cabeçalho `FBV0001` ocupa 8 bytes,
//! o byte 6 indica o tamanho da chave em blocos de 128 bytes e os dados começam
//! em `8 + key_size`, com `data[i] ^= key[i % key_size]`.

use std::collections::HashMap;

use crate::formats::bvr3::assemble_segments;
use crate::formats::text::{decode_text, find_str, parse_f32, split_lines};
use crate::model::{Board, FileFormat, Nail, Part, Pin, Pt, Side};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    None,
    Layout,
    OutlineSegments,
    Overlay,
    Pin,
    Nail,
    Parts,
}

pub fn detect(bytes: &[u8]) -> bool {
    find_str(bytes, "BVRAW_FORMAT_2")
}

pub fn detect_fbv(bytes: &[u8]) -> bool {
    bytes.starts_with(b"FBV000") || find_str(&bytes[..bytes.len().min(64)], "FBV0001")
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    parse_body(bytes, name, FileFormat::Bvr2)
}

pub fn parse_fbv(bytes: &[u8], name: &str) -> Result<Board, String> {
    let data = decrypt_fbv(bytes)?;
    parse_body(&data, name, FileFormat::Fbv)
}

fn decrypt_fbv(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 8 || !bytes.starts_with(b"FBV000") {
        return Err("FBV: cabeçalho inválido".into());
    }
    let key_size = bytes[6] as usize * 128;
    if key_size == 0 || bytes.len() < 8 + key_size {
        return Err(format!(
            "FBV: chave de {} bytes maior que o arquivo",
            key_size
        ));
    }
    let key = &bytes[8..8 + key_size];
    let mut data = bytes[8 + key_size..].to_vec();
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= key[i % key_size];
    }
    Ok(data)
}

fn parse_body(bytes: &[u8], name: &str, format: FileFormat) -> Result<Board, String> {
    let text = decode_text(bytes);
    let mut board = Board::new(name, format);
    let mut section = Section::None;
    let mut pin_part_map: HashMap<String, usize> = HashMap::new();
    let mut current_part: Option<usize> = None;
    let mut pname = String::new();
    let mut pvalue = String::new();
    let mut pmfgcode = String::new();
    let mut layout_segments: Vec<(Pt, Pt)> = Vec::new();
    let mut olsegments: Vec<(Pt, Pt)> = Vec::new();
    let mut overlay: Vec<(Pt, Pt)> = Vec::new();
    let mut saw_any = false;

    for raw_line in split_lines(&text) {
        let line = raw_line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "<<Layout>>" => {
                section = Section::Layout;
                saw_any = true;
                continue;
            }
            "<<olsegments>>" => {
                section = Section::OutlineSegments;
                saw_any = true;
                continue;
            }
            "<<Overlay>>" => {
                section = Section::Overlay;
                saw_any = true;
                continue;
            }
            "<<Pin>>" => {
                section = Section::Pin;
                saw_any = true;
                continue;
            }
            "<<Nail>>" => {
                section = Section::Nail;
                saw_any = true;
                continue;
            }
            "<<Parts>>" => {
                section = Section::Parts;
                saw_any = true;
                continue;
            }
            _ => {}
        }

        match section {
            Section::Layout | Section::OutlineSegments | Section::Overlay => {
                let normalized = line.replace(',', " ");
                let tokens: Vec<&str> = normalized.split_whitespace().collect();
                let nums: Vec<f32> = tokens.iter().take(4).map(|s| parse_f32(s)).collect();
                if nums.len() >= 4 {
                    let a = Pt::new(nums[0] * 1000.0, nums[1] * 1000.0);
                    let b = Pt::new(nums[2] * 1000.0, nums[3] * 1000.0);
                    match section {
                        Section::Layout => layout_segments.push((a, b)),
                        Section::OutlineSegments => olsegments.push((a, b)),
                        Section::Overlay => overlay.push((a, b)),
                        _ => {}
                    }
                } else if nums.len() >= 2 {
                    board
                        .outline
                        .push(Pt::new(nums[0] * 1000.0, nums[1] * 1000.0));
                }
            }
            Section::Nail => {
                let tokens: Vec<&str> = line.split_whitespace().collect();
                if tokens.len() < 2 {
                    continue;
                }
                // O primeiro campo é um índice/placeholder; x/y vêm depois.
                let get = |i: usize| tokens.get(i).copied().unwrap_or("");
                let x = parse_f32(get(1)) * 1000.0;
                let y = parse_f32(get(2)) * 1000.0;
                let _kind = get(3);
                let _grid = get(4);
                let loc = get(5);
                let net = get(7).to_string();
                let side = side_from_location(loc);
                board.nails.push(Nail {
                    pos: Pt::new(x, y),
                    side,
                    net,
                    probe: 0,
                });
            }
            Section::Pin => {
                let split_at = line.find(char::is_whitespace).unwrap_or(line.len());
                let key = &line[..split_at];
                let rest = line[split_at..].trim();
                match key {
                    "PNAME" => {
                        pname = rest.to_string();
                        current_part = None;
                    }
                    "PVALUE" => {
                        pvalue = rest.to_string();
                        if let Some(idx) = current_part {
                            board.parts[idx].value = pvalue.clone();
                        }
                    }
                    "PMFGCODE" => {
                        pmfgcode = rest.to_string();
                        if let Some(idx) = current_part {
                            board.parts[idx].mfgcode = pmfgcode.clone();
                        }
                    }
                    _ if key.starts_with('(') => {
                        let tokens: Vec<&str> = line.split_whitespace().collect();
                        if tokens.len() < 3 {
                            continue;
                        }
                        let loc = tokens[0];
                        let side = side_from_location(loc);
                        let number = tokens[1].to_string();
                        let pin_name = tokens[2].to_string();
                        let x = tokens.get(3).map(|s| parse_f32(s)).unwrap_or(0.0) * 1000.0;
                        let y = tokens.get(4).map(|s| parse_f32(s)).unwrap_or(0.0) * 1000.0;
                        // token 5 = layer
                        let net = tokens.get(6).copied().unwrap_or("").to_string();

                        let part_index = if !pname.is_empty() {
                            if let Some(&idx) = pin_part_map.get(&pname) {
                                idx
                            } else {
                                let mut part = Part::new(pname.clone());
                                part.value = pvalue.clone();
                                part.mfgcode = pmfgcode.clone();
                                part.side = side;
                                board.parts.push(part);
                                let idx = board.parts.len() - 1;
                                pin_part_map.insert(pname.clone(), idx);
                                idx
                            }
                        } else if let Some(idx) = current_part {
                            idx
                        } else {
                            let part_name = format!("?{}", board.parts.len() + 1);
                            let mut part = Part::new(part_name);
                            part.side = side;
                            board.parts.push(part);
                            board.parts.len() - 1
                        };

                        if board.parts[part_index].side == Side::Both {
                            board.parts[part_index].side = side;
                        } else if board.parts[part_index].side != side {
                            board.parts[part_index].side = Side::Both;
                        }
                        current_part = Some(part_index);

                        let mut pin = Pin::new(Pt::new(x, y));
                        pin.part = Some(part_index);
                        pin.number = number;
                        pin.name = pin_name;
                        pin.net = net;
                        pin.side = side;
                        board.pins.push(pin);
                        saw_any = true;
                    }
                    _ => {}
                }
            }
            Section::Parts | Section::None => {}
        }
    }

    if !saw_any {
        return Err("BVR2/FBV: nenhuma seção reconhecida".into());
    }

    if board.outline.is_empty() {
        if !layout_segments.is_empty() {
            board.outline = assemble_segments(layout_segments.clone());
        } else if !olsegments.is_empty() {
            board.outline = assemble_segments(olsegments.clone());
        }
    }
    board.segments.extend(layout_segments);
    board.segments.extend(olsegments);
    board.overlay = overlay;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bvr2_body() {
        let sample = "\
BVRAW_FORMAT_2
<<Layout>>
0 0 1 0
1 0 1 0.8
1 0.8 0 0.8
0 0.8 0 0
<<Pin>>
PNAME U1
PVALUE 74HC00
(T) 1 VCC 0.1 0.1 1 VCC
(T) 2 GND 0.2 0.1 1 GND
<<Nail>>
0 0.5 0.5 0 x (T) 1 GND
";
        let board = parse(sample.as_bytes(), "t").unwrap();
        assert_eq!(board.parts.len(), 1);
        assert_eq!(board.pins.len(), 2);
        assert_eq!(board.nails.len(), 1);
        assert_eq!(board.parts[0].value, "74HC00");
        assert_eq!(board.pins[0].net, "VCC");
        assert!(!board.outline.is_empty());
    }

    #[test]
    fn fbv_zero_key_roundtrip() {
        let body = "\
<<Layout>>
0 0 1 0
1 0 1 0.8
<<Pin>>
PNAME R1
(T) 1 A 0.1 0.1 1 N
";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"FBV0001");
        bytes.push(0);
        let key_size = b'1' as usize * 128;
        bytes.extend(std::iter::repeat_n(0u8, key_size));
        bytes.extend_from_slice(body.as_bytes());
        // A chave é toda zero, então o corpo permanece texto puro.
        let board = parse_fbv(&bytes, "t").unwrap();
        assert_eq!(board.format, FileFormat::Fbv);
        assert_eq!(board.parts.len(), 1);
    }
}
