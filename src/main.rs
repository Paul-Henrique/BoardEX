mod app;
mod formats;
mod model;

use std::path::{Path, PathBuf};

use app::BoardExApp;

fn main() -> eframe::Result {
    let mut args = std::env::args_os().skip(1);
    let mut initial: Option<PathBuf> = None;
    let mut inspect_path: Option<PathBuf> = None;
    let mut fz_key: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            "--inspect" => {
                let Some(path) = args.next() else {
                    eprintln!("uso: BoardEX --inspect <arquivo>");
                    std::process::exit(2);
                };
                inspect_path = Some(PathBuf::from(path));
            }
            "--fz-key" => {
                let Some(key) = args.next() else {
                    eprintln!("uso: BoardEX --fz-key \"0x... 0x...\" [arquivo]");
                    std::process::exit(2);
                };
                fz_key = Some(key.to_string_lossy().into_owned());
            }
            "--fz-key-file" => {
                let Some(path) = args.next() else {
                    eprintln!("uso: BoardEX --fz-key-file <arquivo> [arquivo.fz]");
                    std::process::exit(2);
                };
                match std::fs::read_to_string(PathBuf::from(path)) {
                    Ok(text) => fz_key = Some(text),
                    Err(err) => {
                        eprintln!("erro ao ler o arquivo de chave: {err}");
                        std::process::exit(2);
                    }
                }
            }
            _ => {
                if initial.is_none() {
                    initial = Some(PathBuf::from(arg));
                }
            }
        }
    }

    if let Some(key) = fz_key
        && let Err(err) = formats::fz::set_key(&key)
    {
        eprintln!("erro: {err}");
        std::process::exit(2);
    }

    if let Some(path) = inspect_path {
        inspect(&path);
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "BoardEX",
        options,
        Box::new(move |cc| Ok(Box::new(BoardExApp::new(cc, initial)))),
    )
}

fn print_help() {
    print!(
        "BoardEX — visualizador de boardview

uso:
  BoardEX [arquivo]
  BoardEX --inspect <arquivo>

FZ criptografado:
  BoardEX --fz-key \"0x... 0x... (44 palavras)\" <arquivo.fz>
  BoardEX --fz-key-file <arquivo.conf> <arquivo.fz>
  ou defina a variável de ambiente BOARDEX_FZ_KEY.
  A interface também possui o botão Chave FZ….

Formatos: BRD, BRD2, BVR, BVR2, BVR3, BDV, FBV, FZ
"
    );
}

fn inspect(path: &Path) {
    let path = match formats::resolve_input(path) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("erro: {err}");
            std::process::exit(1);
        }
    };
    match formats::load_file(&path) {
        Ok(board) => {
            println!("arquivo: {}", path.display());
            println!("formato: {}", board.format.label());
            println!(
                "componentes: {}  pinos: {}  nails: {}  nets: {}",
                board.parts.len(),
                board.pins.len(),
                board.nails.len(),
                board.nets.len()
            );
            println!(
                "área: {:.1} × {:.1} mil",
                board.bounds.width(),
                board.bounds.height()
            );
            println!(
                "contorno: {} pontos  segmentos: {}  overlay: {}",
                board.outline.len(),
                board.segments.len(),
                board.overlay.len()
            );
            for warning in &board.warnings {
                println!("aviso: {warning}");
            }
        }
        Err(err) => {
            eprintln!("erro: {err}");
            std::process::exit(1);
        }
    }
}
