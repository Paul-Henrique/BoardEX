//! Formato FZ (ASUS): carga zlib opcionalmente cifrada com RC6.
//!
//! O contêiner termina com o tamanho da seção de descrição em little-endian.
//! A seção de conteúdo começa no byte 4 e contém blocos `A!REFDES`,
//! `A!NET_NAME` e `A!TESTVIA`, com registros `S!` separados por `!`.
//! Arquivos cifrados exigem a chave de 44 palavras de 32 bits fornecida pelo
//! usuário (`BOARDEX_FZ_KEY` ou `--fz-key`); nenhuma chave é embutida aqui.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{OnceLock, RwLock};

use flate2::read::ZlibDecoder;

use crate::formats::text::{decode_text, split_lines};
use crate::model::{BBox, Board, FileFormat, Nail, Part, Pin, Pt, Side};

const KEY_WORDS: usize = 44;
const OUTLINE_MARGIN: f32 = 20.0;

const KEY_PARITY: [u8; KEY_WORDS] = [
    0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0,
    0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1,
];

static KEY_STORE: OnceLock<RwLock<Option<[u32; KEY_WORDS]>>> = OnceLock::new();

struct Payload {
    content: Vec<u8>,
    descr: Vec<u8>,
}

fn key_store() -> &'static RwLock<Option<[u32; KEY_WORDS]>> {
    KEY_STORE.get_or_init(|| RwLock::new(None))
}

fn stored_key() -> Option<[u32; KEY_WORDS]> {
    *key_store()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Define ou substitui a chave FZ para esta execução.
///
/// A troca é permitida para que a interface possa tentar novamente quando o
/// usuário cola uma chave diferente.
pub fn set_key(text: &str) -> Result<(), String> {
    let key = parse_key(text)?;
    if !key_parity_ok(&key) {
        return Err(
            "chave FZ: paridade inválida; confira se a chave foi copiada por completo".into(),
        );
    }
    *key_store()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(key);
    Ok(())
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Board, String> {
    let payload = extract_payload(bytes)?;
    let content = decode_text(&payload.content);
    let descr = decode_text(&payload.descr);
    parse_content(&content, &descr, name)
}

/// Reconhece o contêiner sem tentar decifrar (a extensão `.fz` é o critério
/// principal; isto serve para arquivos sem cifra com outro nome).
pub fn detect(bytes: &[u8]) -> bool {
    bytes.len() > 12
        && bytes[4] == 0x78
        && (bytes[5] == 0x9c || bytes[5] == 0xda)
        && container_length(bytes).is_some_and(|n| n <= bytes.len())
}

fn container_length(bytes: &[u8]) -> Option<usize> {
    let tail = bytes.get(bytes.len().checked_sub(4)?..)?;
    Some(u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]) as usize)
}

fn extract_payload(bytes: &[u8]) -> Result<Payload, String> {
    extract_payload_with_key(bytes, None)
}

fn extract_payload_with_key(
    bytes: &[u8],
    explicit_key: Option<[u32; KEY_WORDS]>,
) -> Result<Payload, String> {
    if bytes.len() < 12 {
        return Err("FZ: arquivo muito curto".into());
    }
    let mut data = bytes.to_vec();
    let is_plain_zlib = data[4] == 0x78 && (data[5] == 0x9c || data[5] == 0xda);
    if !is_plain_zlib {
        let key = match explicit_key {
            Some(key) => key,
            None => configured_key()?.ok_or_else(|| {
                "FZ criptografado: informe a chave de 44 palavras (BOARDEX_FZ_KEY, --fz-key, \
                 --fz-key-file ou o botão Chave FZ…)"
                    .to_string()
            })?,
        };
        if !key_parity_ok(&key) {
            return Err("FZ: chave inválida (paridade não confere)".into());
        }
        rc6_decode(&mut data, &key);
    }

    // O tamanho da descrição também está cifrado; só pode ser lido depois do
    // RC6. A implementação de referência decodifica o arquivo inteiro antes
    // de chamar split().
    let descr_size = container_length(&data)
        .filter(|&n| n > 0 && n <= data.len())
        .ok_or_else(|| "FZ: tamanho da seção de descrição inválido".to_string())?;

    let len = data.len();
    let last_error = if is_plain_zlib {
        format!(
            "FZ: não foi possível descomprimir o conteúdo ({} bytes)",
            len
        )
    } else {
        format!(
            "FZ: não foi possível descomprimir o conteúdo; a chave pode estar incorreta ({} bytes)",
            len
        )
    };

    // A implementação de referência usa `len - descr_size + 4`. Alguns
    // arquivos tratam o campo como o tamanho total da seção; tentamos as duas
    // interpretações antes de desistir.
    let base = len as i64 - descr_size as i64;
    for extra in [4i64, 0, 8, -4] {
        let Some(start) = (base + extra)
            .try_into()
            .ok()
            .filter(|&v: &usize| v > 4 && v < len.saturating_sub(4))
        else {
            continue;
        };
        let Some(content) = inflate(&data[4..start]) else {
            continue;
        };
        let Some(descr) = inflate(&data[start..len - 4]) else {
            continue;
        };
        return Ok(Payload { content, descr });
    }

    Err(last_error)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let mut decoder = ZlibDecoder::new(data);
    let mut output = Vec::new();
    decoder.read_to_end(&mut output).ok()?;
    (!output.is_empty()).then_some(output)
}

fn configured_key() -> Result<Option<[u32; KEY_WORDS]>, String> {
    if let Some(key) = stored_key() {
        return Ok(Some(key));
    }
    // OPENBV_FZ_KEY continua aceito como alias de compatibilidade.
    match std::env::var("BOARDEX_FZ_KEY").or_else(|_| std::env::var("OPENBV_FZ_KEY")) {
        Ok(text) if !text.trim().is_empty() => parse_key(&text).map(Some),
        _ => Ok(None),
    }
}

/// Extrai a chave de um texto bruto ou de um arquivo de configuração no estilo
/// do OpenBoardView (`FZKey = 0x...`). Linhas comentadas são ignoradas.
fn select_key_text(text: &str) -> &str {
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("FZKey"))
        {
            return line[5..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
        }
    }
    text
}

fn parse_key(text: &str) -> Result<[u32; KEY_WORDS], String> {
    let text = select_key_text(text);
    let mut key = [0u32; KEY_WORDS];
    let mut found = 0usize;
    for token in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let token = token
            .trim()
            .rsplit('=')
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| c == '(' || c == ')');
        let token = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
            .unwrap_or(token);
        if token.is_empty() || token.len() > 8 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(value) = u32::from_str_radix(token, 16) else {
            continue;
        };
        if found < KEY_WORDS {
            key[found] = value;
            found += 1;
        }
    }
    if found != KEY_WORDS {
        return Err(format!(
            "chave FZ: esperados {KEY_WORDS} valores hexadecimais de 32 bits, encontrados {found}"
        ));
    }
    Ok(key)
}

fn key_parity_ok(key: &[u32; KEY_WORDS]) -> bool {
    for (word, expected) in key.iter().zip(KEY_PARITY) {
        let mut tmp = *word;
        tmp ^= tmp >> 16;
        tmp ^= tmp >> 8;
        tmp ^= tmp >> 4;
        tmp ^= tmp >> 2;
        tmp ^= tmp >> 1;
        if (((!tmp) & 1) as u8) != expected {
            return false;
        }
    }
    true
}

fn rc6_decode(data: &mut [u8], key: &[u32; KEY_WORDS]) {
    let (mut a, mut b, mut c, mut d) = (0u32, 0u32, 0u32, 0u32);
    let mut stream = [0u8; 16];

    for byte in data.iter_mut() {
        b = b.wrapping_add(key[0]);
        d = d.wrapping_add(key[1]);
        for i in 1..=20 {
            let t = b
                .wrapping_mul(b.wrapping_mul(2).wrapping_add(1))
                .rotate_left(5);
            let u = d
                .wrapping_mul(d.wrapping_mul(2).wrapping_add(1))
                .rotate_left(5);
            a = (a ^ t).rotate_left(u).wrapping_add(key[2 * i]);
            c = (c ^ u).rotate_left(t).wrapping_add(key[2 * i + 1]);
            (a, b, c, d) = (b, c, d, a);
        }
        a = a.wrapping_add(key[42]);
        // `c` não é usado após esta rodada: o estado do próximo byte vem
        // inteiramente do fluxo cifrado (`stream`).

        let current = *byte;
        *byte = current ^ (a as u8);

        stream.copy_within(1.., 0);
        stream[15] = current;
        a = u32::from_le_bytes([stream[0], stream[1], stream[2], stream[3]]);
        b = u32::from_le_bytes([stream[4], stream[5], stream[6], stream[7]]);
        c = u32::from_le_bytes([stream[8], stream[9], stream[10], stream[11]]);
        d = u32::from_le_bytes([stream[12], stream[13], stream[14], stream[15]]);
    }
}

fn parse_content(content: &str, descr: &str, name: &str) -> Result<Board, String> {
    let mut board = Board::new(name, FileFormat::Fz);
    let mut part_map: HashMap<String, usize> = HashMap::new();
    let mut block = 0u8;
    let mut multiplier = 1.0f32;
    let mut saw_block = false;
    let mut missing_parts = 0usize;

    for raw in split_lines(content) {
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("UNIT:millimeters") {
            multiplier = 25.4;
            continue;
        }
        if let Some(rest) = line.strip_prefix("A!") {
            let header = rest.split('!').next().unwrap_or("").trim();
            block = match header {
                "REFDES" => 1,
                "NET_NAME" => 2,
                "TESTVIA" => 3,
                "GRAPHIC_DATA_NAME" => 4,
                "CLASS" => 5,
                "LOGOInfo" => 6,
                "UnDrawSym" => 7,
                _ => 0,
            };
            saw_block = true;
            continue;
        }
        let Some(data) = line.strip_prefix("S!") else {
            continue;
        };
        match block {
            1 => {
                let fields = fields(data);
                let part_name = field(&fields, 0);
                if part_name.is_empty() {
                    continue;
                }
                let side = if field(&fields, 3).eq_ignore_ascii_case("YES") {
                    Side::Bottom
                } else {
                    Side::Top
                };
                if let Some(&idx) = part_map.get(part_name) {
                    board.parts[idx].side = side;
                } else {
                    let mut part = Part::new(part_name.to_string());
                    part.side = side;
                    part.kind = crate::model::PartKind::Smd;
                    let idx = board.parts.len();
                    board.parts.push(part);
                    part_map.insert(part_name.to_string(), idx);
                }
            }
            2 => {
                let fields = fields(data);
                let net = field(&fields, 0).to_string();
                let part_name = field(&fields, 1);
                let number = field(&fields, 2);
                let pin_name = field(&fields, 3);
                let x = parse_number(field(&fields, 4)) * multiplier;
                let y = parse_number(field(&fields, 5)) * multiplier;
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                let probe = parse_integer(field(&fields, 6));
                let radius = (parse_number(field(&fields, 7)) / 100.0).max(0.5) * multiplier;

                let part_index = if let Some(&idx) = part_map.get(part_name) {
                    idx
                } else if part_name.is_empty() {
                    continue;
                } else {
                    let mut part = Part::new(format!("?{part_name}"));
                    part.side = Side::Both;
                    let idx = board.parts.len();
                    board.parts.push(part);
                    part_map.insert(part_name.to_string(), idx);
                    missing_parts += 1;
                    idx
                };

                let mut pin = Pin::new(Pt::new(x, y));
                pin.net = net;
                pin.part = Some(part_index);
                pin.probe = probe;
                pin.radius = radius;
                if number.is_empty() || number == "0" {
                    pin.name = pin_name.to_string();
                } else {
                    pin.number = number.to_string();
                    pin.name = pin_name.to_string();
                }
                board.pins.push(pin);
            }
            3 => {
                let data = data.strip_prefix("Y!").unwrap_or(data);
                let fields = fields(data);
                let x = parse_number(field(&fields, 4)) * multiplier;
                let y = parse_number(field(&fields, 5)) * multiplier;
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                let location = field(&fields, 6);
                board.nails.push(Nail {
                    pos: Pt::new(x, y),
                    side: if location == "T" {
                        Side::Top
                    } else {
                        Side::Bottom
                    },
                    net: field(&fields, 0).to_string(),
                    probe: parse_integer(field(&fields, 2)),
                });
            }
            _ => {}
        }
    }

    if !saw_block {
        return Err("FZ: conteúdo não contém blocos A!REFDES/A!NET_NAME".into());
    }
    if missing_parts > 0 {
        board.warnings.push(format!(
            "FZ: {missing_parts} componente(s) referenciado(s) por pinos sem bloco REFDES"
        ));
    }

    // A chave não é necessária aqui; a seção de descrição é opcional.
    apply_descriptions(&mut board, &part_map, descr);

    // Mantém o lado dos pinos coerente mesmo quando o REFDES veio depois.
    for pin in &mut board.pins {
        if let Some(idx) = pin.part
            && let Some(part) = board.parts.get(idx)
        {
            pin.side = part.side;
        }
    }

    if !board.pins.is_empty() {
        let mut b = BBox::empty();
        for pin in &board.pins {
            b.add(pin.pos);
        }
        for nail in &board.nails {
            b.add(nail.pos);
        }
        if !b.is_empty() {
            board.outline = vec![
                Pt::new(b.min.x - OUTLINE_MARGIN, b.min.y - OUTLINE_MARGIN),
                Pt::new(b.max.x + OUTLINE_MARGIN, b.min.y - OUTLINE_MARGIN),
                Pt::new(b.max.x + OUTLINE_MARGIN, b.max.y + OUTLINE_MARGIN),
                Pt::new(b.min.x - OUTLINE_MARGIN, b.max.y + OUTLINE_MARGIN),
                Pt::new(b.min.x - OUTLINE_MARGIN, b.min.y - OUTLINE_MARGIN),
            ];
        }
    }

    board.finalize();
    Ok(board)
}

fn apply_descriptions(board: &mut Board, part_map: &HashMap<String, usize>, descr: &str) {
    for line in split_lines(descr).into_iter().skip(2) {
        let line = line.trim_end_matches(['\0', '\r', '\n']);
        if line.trim().is_empty() || line.trim_start().starts_with('s') || !line.contains('\t') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let description = field(&fields, 1);
        if description.is_empty() {
            continue;
        }
        let partno2 = field(&fields, 4);
        for location in field(&fields, 3).split_whitespace() {
            if let Some(&idx) = part_map.get(location) {
                let part = &mut board.parts[idx];
                part.value = description.to_string();
                if !partno2.is_empty() {
                    part.mfgcode = partno2.to_string();
                }
            }
        }
    }
}

fn fields(data: &str) -> Vec<&str> {
    data.split('!')
        .map(|s| s.trim_matches(|c: char| c == '\0' || c.is_whitespace()))
        .collect()
}

fn field<'a>(fields: &'a [&str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("")
}

fn parse_number(text: &str) -> f32 {
    text.replace(',', ".").parse::<f32>().unwrap_or(0.0)
}

fn parse_integer(text: &str) -> i32 {
    text.parse::<i32>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::write::ZlibEncoder;

    use super::*;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn container(content: &str, descr: &str) -> Vec<u8> {
        let content = zlib(content.as_bytes());
        let descr = zlib(descr.as_bytes());
        let mut file = b"FZ01".to_vec();
        file.extend_from_slice(&content);
        file.extend_from_slice(&descr);
        file.extend_from_slice(&(descr.len() as u32 + 8).to_le_bytes());
        file
    }

    const TEST_KEY: &str = "\
0x7d962443, 0xba9219ad, 0x7cab306c, 0x4016f4de, 0x9982207c, 0x078cb4bc, 0x8e581344, 0xcf962720,
0xc3435a6a, 0x12f52ff0, 0xacb044ab, 0x7eb7fd59, 0x8c87f10e, 0x5d284d0d, 0xc427ba8b, 0xcf92f770,
0x616fa629, 0x20b241a2, 0xd6aec18e, 0xe9de91f3, 0xec9e2d13, 0x5b0836c8, 0x7816cb64, 0x501851e6,
0x068e68b3, 0x03777d65, 0x8172a4dd, 0x61073a20, 0x4b6405db, 0x2ec893ad, 0x60d21886, 0x1a988701,
0x9018d08e, 0xc9205dee, 0x5566c174, 0x85f74413, 0xb51134d0, 0xeaba207a, 0x95551b2c, 0xb128d119,
0x4991b58c, 0x723ba85a, 0x8d40d7a3, 0xd7c74065";

    // Contêiner cifrado gerado com uma implementação independente do RC6
    // (porte literal da referência), para regressão do caminho cifrado.
    const TEST_ENCRYPTED: &str = "\
99d68e42dd41eec19a144f7ce88e3a29d18dc38bc835aa6bc2df908668e13a67\
e9c054f4b5bfa735ac8fece39a22335157d8575c30f505316a5b10354f88610f\
c68a36a85cd5d5594715c4f41ebf365067151e0c0a83ca9f9abe";

    fn from_hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn parse_unencrypted_fz() {
        let content = "\
A!REFDES!
S!R1!C1!S1!NO!0!
S!U1!C2!S2!YES!0!
A!NET_NAME!
S!GND!R1!1!GND!100!100!0!6!
S!VCC!U1!0!A1!200!100!0!6!
S!GND!U1!2!GND!200!200!0!6!
A!TESTVIA!
S!Y!GND!U1!3!GND!150!150!T!0.5!
";
        let descr = "\
Placa de teste
PartNo\tDescription\tQty\tLocations\tPartNo2
\tRES 10K 1%\t1\tR1\tRC0402
";
        let board = parse(&container(content, descr), "demo").unwrap();
        assert_eq!(board.format, FileFormat::Fz);
        assert_eq!(board.parts.len(), 2);
        assert_eq!(board.pins.len(), 3);
        assert_eq!(board.nails.len(), 1);
        assert_eq!(board.pins[0].number, "1");
        assert_eq!(board.pins[1].name, "A1");
        assert_eq!(board.nails[0].net, "GND");
        assert_eq!(board.parts[0].value, "RES 10K 1%");
        assert_eq!(board.parts[0].mfgcode, "RC0402");
        assert_eq!(board.outline.len(), 5);
        assert!(board.nets.iter().any(|net| net == "VCC"));
    }

    #[test]
    fn parse_millimeter_units() {
        let content = "\
UNIT:millimeters
A!REFDES!
S!U1!C1!S1!NO!0!
A!NET_NAME!
S!N1!U1!1!N1!10!20!0!1!
";
        let board = parse(&container(content, "desc\ncols\n"), "mm").unwrap();
        assert!((board.pins[0].pos.x - 254.0).abs() < 0.01);
        assert!((board.pins[0].pos.y - 508.0).abs() < 0.01);
    }

    #[test]
    fn encrypted_fz_fixture_matches_reference() {
        let key = parse_key(TEST_KEY).unwrap();
        assert!(key_parity_ok(&key));
        let payload = extract_payload_with_key(&from_hex(TEST_ENCRYPTED), Some(key)).unwrap();
        let content = decode_text(&payload.content);
        let descr = decode_text(&payload.descr);
        let board = parse_content(&content, &descr, "enc").unwrap();
        assert_eq!(board.parts.len(), 1);
        assert_eq!(board.pins.len(), 1);
        assert_eq!(board.parts[0].name, "U1");
        assert_eq!(board.pins[0].net, "GND");
    }

    #[test]
    fn parse_fz_key() {
        let text = (0..KEY_WORDS)
            .map(|i| format!("0x{:08x}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let key = parse_key(&text).unwrap();
        assert_eq!(key[0], 1);
        assert_eq!(key[43], 44);

        let config = format!("dpi = 96\n# FZKey = 0xdeadbeef\nFZKey = {text}\n");
        assert_eq!(parse_key(&config).unwrap(), key);
        assert!(parse_key("0x1, 0x2").is_err());
    }

    #[test]
    fn invalid_container_is_rejected() {
        assert!(parse(b"FZ01short", "x").is_err());
    }
}
