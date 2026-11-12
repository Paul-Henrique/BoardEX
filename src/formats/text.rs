//! Utilitários de leitura de arquivos texto usados pelos formatos de boardview.

/// Decodifica bytes como UTF-8; se falhar, interpreta como Windows-1252.
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| cp1252_char(b)).collect(),
    }
}

fn cp1252_char(b: u8) -> char {
    match b {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        _ => b as char,
    }
}

/// Divide em linhas removendo `\r` final. Mantém linhas vazias.
pub fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Tokenizador simples por espaços/tabs, no estilo dos parsers C.
pub struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    pub fn new(s: &'a str) -> Self {
        Self { rest: s }
    }

    pub fn next(&mut self) -> Option<&'a str> {
        let s = self.rest.trim_start();
        if s.is_empty() {
            self.rest = "";
            return None;
        }
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        let token = &s[..end];
        self.rest = &s[end..];
        Some(token)
    }

    pub fn next_str(&mut self) -> &'a str {
        self.next().unwrap_or("")
    }

    pub fn next_f32(&mut self) -> f32 {
        self.next().map(parse_f32).unwrap_or(0.0)
    }

    pub fn next_i32(&mut self) -> i32 {
        self.next().map(parse_i32).unwrap_or(0)
    }

    pub fn next_u32(&mut self) -> u32 {
        self.next_i32().max(0) as u32
    }

    pub fn is_empty(&self) -> bool {
        self.rest.trim().is_empty()
    }
}

/// Aceita `12`, `-3`, `+4.5`, `1,5` e tokens com lixo no final.
pub fn parse_f32(token: &str) -> f32 {
    let cleaned: String = token
        .chars()
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    if let Ok(v) = cleaned.parse::<f32>() {
        return v;
    }
    let mut end = 0;
    let mut seen_dot = false;
    for (i, c) in cleaned.char_indices() {
        let numeric =
            c.is_ascii_digit() || ((i == 0) && (c == '-' || c == '+')) || (c == '.' && !seen_dot);
        if c == '.' {
            seen_dot = true;
        }
        if !numeric {
            break;
        }
        end = i + c.len_utf8();
    }
    cleaned[..end].parse::<f32>().unwrap_or(0.0)
}

pub fn parse_i32(token: &str) -> i32 {
    parse_f32(token).trunc() as i32
}

pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

pub fn find_str(haystack: &[u8], needle: &str) -> bool {
    find_subslice(haystack, needle.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_basic() {
        let mut t = Tokens::new("  U1\t(T) 12-3  4.5 6 lixo");
        assert_eq!(t.next(), Some("U1"));
        assert_eq!(t.next(), Some("(T)"));
        assert_eq!(t.next_i32(), 12);
        assert_eq!(t.next_f32(), 4.5);
        assert_eq!(t.next_f32(), 6.0);
        assert_eq!(t.next(), Some("lixo"));
        assert_eq!(t.next(), None);
    }

    #[test]
    fn cp1252_fallback() {
        let s = decode_text(&[b'R', 0xE9, b'g']);
        assert_eq!(s, "Rég");
    }
}
