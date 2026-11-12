//! Detecção e leitura dos formatos de boardview suportados.

pub mod bdv;
pub mod brd;
pub mod brd2;
pub mod bvr;
pub mod bvr2;
pub mod bvr3;
pub mod fz;
pub mod text;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::model::Board;

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Unsupported(String),
    Parse(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "não foi possível ler o arquivo: {e}"),
            Self::Unsupported(msg) => write!(f, "{msg}"),
            Self::Parse(msg) => write!(f, "erro ao interpretar a placa: {msg}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Aceita um arquivo diretamente ou uma pasta que contenha exatamente um `.fz`.
///
/// Isso cobre o caso comum de baixar um pacote `.fz` que vem em uma pasta com
/// o mesmo nome. Se houver mais de um `.fz`, a escolha é explícita.
pub fn resolve_input(path: &Path) -> Result<PathBuf, LoadError> {
    if !path.is_dir() {
        return Ok(path.to_path_buf());
    }

    let mut candidates = Vec::new();
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > 4 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if depth == 0 => return Err(err.into()),
            Err(_) => continue,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let candidate = entry.path();
            if file_type.is_file()
                && candidate
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("fz"))
            {
                candidates.push(candidate);
            } else if file_type.is_dir() {
                pending.push((candidate, depth + 1));
            }
        }
    }
    candidates.sort();

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(LoadError::Unsupported(format!(
            "a pasta não contém nenhum arquivo .fz: {}",
            path.display()
        ))),
        _ => {
            let names = candidates
                .iter()
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
                .collect::<Vec<_>>()
                .join(" | ");
            Err(LoadError::Unsupported(format!(
                "a pasta contém vários arquivos .fz; abra um deles diretamente: {names}"
            )))
        }
    }
}

pub fn load_file(path: &Path) -> Result<Board, LoadError> {
    let path = resolve_input(path)?;
    let bytes = std::fs::read(&path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("placa")
        .to_string();
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fz"))
    {
        return fz::parse(&bytes, &name).map_err(LoadError::Parse);
    }
    load(&bytes, &name)
}

pub fn load(bytes: &[u8], name: &str) -> Result<Board, LoadError> {
    if bytes.is_empty() {
        return Err(LoadError::Parse("arquivo vazio".into()));
    }

    if bvr2::detect_fbv(bytes) {
        return bvr2::parse_fbv(bytes, name).map_err(LoadError::Parse);
    }
    if bvr3::detect(bytes) {
        return bvr3::parse(bytes, name).map_err(LoadError::Parse);
    }
    if bvr2::detect(bytes) {
        return bvr2::parse(bytes, name).map_err(LoadError::Parse);
    }
    if bvr::detect(bytes) {
        return bvr::parse(bytes, name).map_err(LoadError::Parse);
    }
    if bdv::detect(bytes) {
        return bdv::parse(bytes, name).map_err(LoadError::Parse);
    }
    if brd2::detect(bytes) {
        return brd2::parse(bytes, name).map_err(LoadError::Parse);
    }
    if brd::detect(bytes) {
        return brd::parse(bytes, name).map_err(LoadError::Parse);
    }
    if fz::detect(bytes) {
        return fz::parse(bytes, name).map_err(LoadError::Parse);
    }

    let preview = text::decode_text(&bytes[..bytes.len().min(48)]);
    let preview: String = preview
        .chars()
        .map(|c| if c.is_control() { '·' } else { c })
        .collect();
    Err(LoadError::Unsupported(format!(
        "formato não reconhecido nesta versão. Suportados: BRD, BRD2, BVR, BVR2, BVR3, BDV, FBV e FZ.\nPrimeiros bytes: {preview}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_folder_with_single_fz() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("boardex-resolve-{}-{stamp}", std::process::id()));
        let nested = dir.join("pacote.fz");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("placa.FZ");
        std::fs::write(&file, b"conteudo").unwrap();

        assert_eq!(resolve_input(&dir).unwrap(), file);
        assert_eq!(resolve_input(&nested).unwrap(), file);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detect_each_format_header() {
        assert_eq!(
            load(
                b"BRDOUT: 2 100 100\nNETS: 0\nPARTS: 0\nPINS: 0\nNAILS: 0\n",
                "x"
            )
            .unwrap()
            .format,
            crate::model::FileFormat::Brd2
        );
        assert_eq!(
            load(
                b"BVRAW_FORMAT_1\n<<Layout>>\nheader\n0 0\n1 0\n1 1\n0 1\n<<Pin>>\nheader\n",
                "x"
            )
            .unwrap()
            .format,
            crate::model::FileFormat::Bvr
        );
    }
}
