//! Interface do BoardEX: câmera pan/zoom, renderização e busca.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Pos2, Rect, Sense, Shape,
    Stroke, StrokeKind, Vec2,
};

use crate::formats;
use crate::model::{BBox, Board, FileFormat, Pin, Pt, Side};

const BG: Color32 = Color32::from_rgb(9, 12, 16);
const BOARD_FILL: Color32 = Color32::from_rgb(23, 30, 38);
const OUTLINE: Color32 = Color32::from_rgb(122, 143, 168);
const PART_TOP: Color32 = Color32::from_rgb(48, 82, 115);
const PART_TOP_STROKE: Color32 = Color32::from_rgb(94, 143, 186);
const PART_BOTTOM: Color32 = Color32::from_rgb(92, 61, 42);
const PART_BOTTOM_STROKE: Color32 = Color32::from_rgb(161, 109, 73);
const PART_BOTH: Color32 = Color32::from_rgb(55, 83, 64);
const PART_BOTH_STROKE: Color32 = Color32::from_rgb(104, 148, 112);
const PIN_TOP: Color32 = Color32::from_rgb(235, 112, 92);
const PIN_BOTTOM: Color32 = Color32::from_rgb(84, 166, 235);
const PIN_BOTH: Color32 = Color32::from_rgb(120, 199, 132);
const VOLT_12_20: Color32 = Color32::from_rgb(255, 214, 64);
const VOLT_5: Color32 = Color32::from_rgb(235, 64, 64);
const VOLT_3V3: Color32 = Color32::from_rgb(190, 90, 255);
const VOLT_1V8: Color32 = Color32::from_rgb(52, 152, 235);
const VOLT_1V: Color32 = Color32::from_rgb(245, 245, 245);
const NET_HIGHLIGHT: Color32 = Color32::from_rgb(46, 160, 88);
const PART_SELECT: Color32 = Color32::from_rgb(125, 225, 255);
const TEXT: Color32 = Color32::from_rgb(224, 230, 238);
const MUTED: Color32 = Color32::from_rgb(139, 151, 166);
const ACCENT: Color32 = Color32::from_rgb(68, 170, 255);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewSide {
    Top,
    Bottom,
    Both,
}

impl ViewSide {
    fn side(self) -> Side {
        match self {
            Self::Top => Side::Top,
            Self::Bottom => Side::Bottom,
            Self::Both => Side::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoltageRail {
    V12to20,
    V5,
    V3V3,
    V1V8,
    V1,
}

impl VoltageRail {
    fn color(self) -> Color32 {
        match self {
            Self::V12to20 => VOLT_12_20,
            Self::V5 => VOLT_5,
            Self::V3V3 => VOLT_3V3,
            Self::V1V8 => VOLT_1V8,
            Self::V1 => VOLT_1V,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::V12to20 => "12–20 V",
            Self::V5 => "5 V",
            Self::V3V3 => "3,3 V",
            Self::V1V8 => "1,8 V",
            Self::V1 => "1 V",
        }
    }
}

const VOLTAGE_RAILS: [VoltageRail; 5] = [
    VoltageRail::V12to20,
    VoltageRail::V5,
    VoltageRail::V3V3,
    VoltageRail::V1V8,
    VoltageRail::V1,
];

fn rail_for_voltage(voltage: f32) -> Option<VoltageRail> {
    if (12.0..=20.0).contains(&voltage) {
        Some(VoltageRail::V12to20)
    } else if (voltage - 5.0).abs() <= 0.051 {
        Some(VoltageRail::V5)
    } else if (voltage - 3.3).abs() <= 0.051 {
        Some(VoltageRail::V3V3)
    } else if (voltage - 1.8).abs() <= 0.051 {
        Some(VoltageRail::V1V8)
    } else if (voltage - 1.0).abs() <= 0.051 {
        Some(VoltageRail::V1)
    } else {
        None
    }
}

/// Reconhece tensões em nomes de net como `+12V`, `+5V0`, `3V3_S5`,
/// `VCC1V8`, `PP1V0` e `3.3V`. Nomes sem tensão explícita retornam `None`.
fn classify_net_voltage(net: &str) -> Option<VoltageRail> {
    let text = net.to_ascii_uppercase();
    let bytes = text.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if byte != b'V' {
            continue;
        }

        let mut whole_end = i;
        while whole_end > 0 && bytes[whole_end - 1].is_ascii_whitespace() {
            whole_end -= 1;
        }

        let mut whole_start = whole_end;
        while whole_start > 0
            && (bytes[whole_start - 1].is_ascii_digit()
                || bytes[whole_start - 1] == b'.'
                || bytes[whole_start - 1] == b',')
        {
            whole_start -= 1;
        }
        if whole_start == whole_end {
            continue;
        }

        let whole_text = text[whole_start..whole_end].replace(',', ".");
        let Ok(whole) = whole_text.parse::<f32>() else {
            continue;
        };

        let mut fraction_end = i + 1;
        while fraction_end < bytes.len() && bytes[fraction_end].is_ascii_digit() {
            fraction_end += 1;
        }

        let voltage = if fraction_end == i + 1 {
            whole
        } else {
            let fraction_text = &text[i + 1..fraction_end];
            let Ok(fraction) = fraction_text.parse::<f32>() else {
                continue;
            };
            let scale = 10f32.powi((fraction_end - i - 1) as i32);
            whole + fraction / scale
        };

        if let Some(rail) = rail_for_voltage(voltage) {
            return Some(rail);
        }
    }

    None
}

fn side_color(side: Side) -> Color32 {
    match side {
        Side::Top => PIN_TOP,
        Side::Bottom => PIN_BOTTOM,
        Side::Both => PIN_BOTH,
    }
}

fn pin_color(net: &str, side: Side) -> Color32 {
    classify_net_voltage(net)
        .map(VoltageRail::color)
        .unwrap_or_else(|| side_color(side))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComponentType {
    Capacitor,
    Resistor,
    Inductor,
    Transistor,
    Mosfet,
    TransistorMosfet,
    Diode,
    Led,
    Ic,
    CrystalOscillator,
    Fuse,
    Battery,
    Connector,
    Switch,
    Jumper,
    Relay,
    Speaker,
    Antenna,
    TestPoint,
    Motor,
    Other,
}

impl ComponentType {
    fn label(self) -> &'static str {
        match self {
            Self::Capacitor => "Capacitor",
            Self::Resistor => "Resistor",
            Self::Inductor => "Indutor / bobina",
            Self::Transistor => "Transistor",
            Self::Mosfet => "MOSFET",
            Self::TransistorMosfet => "Transistor / MOSFET",
            Self::Diode => "Diodo",
            Self::Led => "LED",
            Self::Ic => "Chip / CI",
            Self::CrystalOscillator => "Cristal / oscilador",
            Self::Fuse => "Fusível",
            Self::Battery => "Bateria",
            Self::Connector => "Conector",
            Self::Switch => "Chave / botão",
            Self::Jumper => "Jumper",
            Self::Relay => "Relé",
            Self::Speaker => "Alto-falante / buzzer",
            Self::Antenna => "Antena",
            Self::TestPoint => "Ponto de teste",
            Self::Motor => "Motor",
            Self::Other => "Outro / não identificado",
        }
    }
}

/// Classifica o componente pelo designador (C1, R2, Q3, U4...) e, se necessário,
/// por palavras no nome. É uma heurística: boardviews nem sempre trazem o tipo.
fn classify_component(name: &str) -> ComponentType {
    let clean = name
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_uppercase();
    let prefix: String = clean
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();

    let mapped = match prefix.as_str() {
        "C" | "CAP" | "CAPACITOR" => Some(ComponentType::Capacitor),
        "R" | "RES" | "RESISTOR" | "RESISTENCIA" | "RN" => Some(ComponentType::Resistor),
        "L" | "IND" | "INDUCTOR" | "BOBINA" | "FB" | "FERRITE" => Some(ComponentType::Inductor),
        "T" | "TR" | "TRANS" | "TRANSISTOR" => Some(ComponentType::Transistor),
        "MOS" | "MOSFET" | "FET" => Some(ComponentType::Mosfet),
        "Q" => Some(ComponentType::TransistorMosfet),
        "D" | "DIODE" | "DIODO" => Some(ComponentType::Diode),
        "LED" => Some(ComponentType::Led),
        "U" | "IC" | "CI" | "CHIP" | "MCU" | "CPU" | "GPU" | "PCH" | "EC" | "PMIC" => {
            Some(ComponentType::Ic)
        }
        "Y" | "X" | "XTAL" | "CRYSTAL" | "CRISTAL" | "OSC" | "OSCILLATOR" | "OSCILADOR" | "XO" => {
            Some(ComponentType::CrystalOscillator)
        }
        "F" | "FUSE" | "FUSIVEL" => Some(ComponentType::Fuse),
        "BT" | "BAT" | "BATT" | "BATTERY" | "BATERIA" => Some(ComponentType::Battery),
        "J" | "CN" | "CON" | "CONN" | "CONNECTOR" | "CONECTOR" | "HEADER" | "SOCKET" | "P"
        | "PLUG" => Some(ComponentType::Connector),
        "SW" | "S" | "SWITCH" | "BUTTON" | "BOTAO" => Some(ComponentType::Switch),
        "JP" | "JMP" | "JUMPER" => Some(ComponentType::Jumper),
        "K" | "RELAY" | "RELE" => Some(ComponentType::Relay),
        "LS" | "SPK" | "SPEAKER" | "BUZZER" | "ALTOFALANTE" => Some(ComponentType::Speaker),
        "ANT" | "ANTENNA" | "ANTENA" => Some(ComponentType::Antenna),
        "TP" | "TEST" | "TESTPOINT" => Some(ComponentType::TestPoint),
        "M" | "MOTOR" => Some(ComponentType::Motor),
        _ => None,
    };
    if let Some(component) = mapped {
        return component;
    }

    let keywords = [
        ("CAPACITOR", ComponentType::Capacitor),
        ("RESISTOR", ComponentType::Resistor),
        ("INDUCTOR", ComponentType::Inductor),
        ("BOBINA", ComponentType::Inductor),
        ("TRANSISTOR", ComponentType::Transistor),
        ("MOSFET", ComponentType::Mosfet),
        ("DIODE", ComponentType::Diode),
        ("DIODO", ComponentType::Diode),
        ("LED", ComponentType::Led),
        ("CHIP", ComponentType::Ic),
        ("CRYSTAL", ComponentType::CrystalOscillator),
        ("CRISTAL", ComponentType::CrystalOscillator),
        ("OSCILLATOR", ComponentType::CrystalOscillator),
        ("OSCILADOR", ComponentType::CrystalOscillator),
        ("FUSE", ComponentType::Fuse),
        ("FUSIVEL", ComponentType::Fuse),
        ("BATTERY", ComponentType::Battery),
        ("BATERIA", ComponentType::Battery),
        ("CONNECTOR", ComponentType::Connector),
        ("CONECTOR", ComponentType::Connector),
        ("SWITCH", ComponentType::Switch),
        ("BUTTON", ComponentType::Switch),
        ("BOTAO", ComponentType::Switch),
        ("JUMPER", ComponentType::Jumper),
        ("RELAY", ComponentType::Relay),
        ("RELE", ComponentType::Relay),
        ("SPEAKER", ComponentType::Speaker),
        ("BUZZER", ComponentType::Speaker),
        ("ANTENNA", ComponentType::Antenna),
        ("ANTENA", ComponentType::Antenna),
        ("TESTPOINT", ComponentType::TestPoint),
        ("MOTOR", ComponentType::Motor),
    ];
    for (needle, component) in keywords {
        if clean.contains(needle) {
            return component;
        }
    }

    ComponentType::Other
}

#[derive(Clone, Debug)]
enum Hit {
    Part(usize),
    Pin(usize),
    Net(String),
}

#[derive(Clone, Debug)]
struct SearchRow {
    hit: Hit,
    label: String,
    detail: String,
}

struct Camera {
    scale: f32,
    offset: Vec2,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            scale: 0.15,
            offset: Vec2::ZERO,
        }
    }
}

pub struct BoardExApp {
    board: Option<Board>,
    board_path: Option<PathBuf>,
    camera: Camera,
    view_side: ViewSide,
    show_names: bool,
    show_pins: bool,
    show_details: bool,
    selected_part: Option<usize>,
    selected_pin: Option<usize>,
    selected_net: Option<String>,
    hovered_pin: Option<usize>,
    query: String,
    hits: Vec<SearchRow>,
    error: Option<String>,
    status: String,
    needs_fit: bool,
    focus_search: bool,
    pending_focus: Option<Hit>,
    last_view_rect: Rect,
    show_fz_key_dialog: bool,
    fz_key_input: String,
    fz_key_error: Option<String>,
    retry_path: Option<PathBuf>,
}

impl BoardExApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        configure_style(&cc.egui_ctx);
        let mut app = Self {
            board: None,
            board_path: None,
            camera: Camera::default(),
            view_side: ViewSide::Top,
            show_names: true,
            show_pins: true,
            show_details: true,
            selected_part: None,
            selected_pin: None,
            selected_net: None,
            hovered_pin: None,
            query: String::new(),
            hits: Vec::new(),
            error: None,
            status: "Arraste um arquivo de boardview ou use Abrir.".to_string(),
            needs_fit: false,
            focus_search: false,
            pending_focus: None,
            last_view_rect: Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
            show_fz_key_dialog: false,
            fz_key_input: String::new(),
            fz_key_error: None,
            retry_path: None,
        };
        if let Some(path) = initial {
            app.open_path(path);
        }
        app
    }

    fn open_dialog(&mut self) {
        let extensions: Vec<&str> = [
            FileFormat::Brd,
            FileFormat::Brd2,
            FileFormat::Bvr,
            FileFormat::Bvr2,
            FileFormat::Bvr3,
            FileFormat::Bdv,
            FileFormat::Fbv,
            FileFormat::Fz,
        ]
        .iter()
        .map(|format| format.extension())
        .collect();
        let picked = rfd::FileDialog::new()
            .add_filter("Boardview", &extensions)
            .add_filter("Todos os arquivos", &["*"])
            .pick_file();
        if let Some(path) = picked {
            self.open_path(path);
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let resolved = match formats::resolve_input(&path) {
            Ok(path) => path,
            Err(err) => {
                self.error = Some(format!("{}: {err}", path.display()));
                return;
            }
        };

        match formats::load_file(&resolved) {
            Ok(board) => {
                let summary = format!(
                    "{}: {} componentes, {} pinos, {} nails, {} nets",
                    board.format.label(),
                    board.parts.len(),
                    board.pins.len(),
                    board.nails.len(),
                    board.nets.len()
                );
                self.status = summary;
                self.board = Some(board);
                self.board_path = Some(resolved);
                self.selected_part = None;
                self.selected_pin = None;
                self.selected_net = None;
                self.hovered_pin = None;
                self.query.clear();
                self.hits.clear();
                self.error = None;
                self.needs_fit = true;
                self.pending_focus = None;
                self.retry_path = None;
                self.fz_key_error = None;
                self.show_fz_key_dialog = false;
            }
            Err(err) => {
                let message = format!("{}: {err}", resolved.display());
                let needs_key = message.contains("FZ")
                    && (message.contains("chave") || message.contains("criptograf"));
                self.error = Some(message);
                if needs_key {
                    self.retry_path = Some(resolved);
                    self.show_fz_key_dialog = true;
                }
            }
        }
    }

    fn update_search(&mut self) {
        self.hits.clear();
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        let Some(board) = self.board.as_ref() else {
            return;
        };

        for (idx, part) in board.parts.iter().enumerate() {
            if part.name.to_lowercase().contains(&query) {
                self.hits.push(SearchRow {
                    hit: Hit::Part(idx),
                    label: format!("{}  ·  {}", part.name, part.side.label()),
                    detail: format!(
                        "{} · {} · {} pino(s){}",
                        classify_component(&part.name).label(),
                        part.kind.label(),
                        part.pins.len(),
                        if part.value.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", part.value)
                        }
                    ),
                });
                if self.hits.len() >= 120 {
                    break;
                }
            }
        }

        let mut seen_nets: HashSet<String> = HashSet::new();
        for (idx, pin) in board.pins.iter().enumerate() {
            if pin.net.to_lowercase().contains(&query)
                && !pin.net.is_empty()
                && seen_nets.insert(pin.net.to_lowercase())
            {
                self.hits.push(SearchRow {
                    hit: Hit::Net(pin.net.clone()),
                    label: format!("NET  {}", pin.net),
                    detail: "destacar e enquadrar a net".to_string(),
                });
                if self.hits.len() >= 220 {
                    break;
                }
            }
            if pin.number.to_lowercase().contains(&query)
                || pin.name.to_lowercase().contains(&query)
            {
                let part = pin.part.map(|i| board.part_name(i)).unwrap_or("(avulso)");
                self.hits.push(SearchRow {
                    hit: Hit::Pin(idx),
                    label: format!("{}  ·  pino {}", part, pin.label()),
                    detail: format!("net: {}", if pin.net.is_empty() { "—" } else { &pin.net }),
                });
                if self.hits.len() >= 320 {
                    break;
                }
            }
        }

        for nail in &board.nails {
            if nail.net.to_lowercase().contains(&query)
                && !nail.net.is_empty()
                && seen_nets.insert(nail.net.to_lowercase())
            {
                self.hits.push(SearchRow {
                    hit: Hit::Net(nail.net.clone()),
                    label: format!("NET  {}", nail.net),
                    detail: "destacar e enquadrar a net (nail/test point)".to_string(),
                });
                if self.hits.len() >= 360 {
                    break;
                }
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (open, search, esc) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::O),
                i.modifiers.command && i.key_pressed(Key::F),
                i.key_pressed(Key::Escape),
            )
        });
        if open {
            self.open_dialog();
        }
        if search {
            self.focus_search = true;
        }
        if esc {
            self.selected_part = None;
            self.selected_pin = None;
            self.selected_net = None;
            self.pending_focus = None;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (fit, top, bottom, both, names, pins, zoom_in, zoom_out) = ctx.input(|i| {
            (
                i.key_pressed(Key::F) || i.key_pressed(Key::Home),
                i.key_pressed(Key::T),
                i.key_pressed(Key::B),
                i.key_pressed(Key::A),
                i.key_pressed(Key::N),
                i.key_pressed(Key::P),
                i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals),
                i.key_pressed(Key::Minus),
            )
        });
        if fit {
            self.needs_fit = true;
        }
        if top {
            self.view_side = ViewSide::Top;
        }
        if bottom {
            self.view_side = ViewSide::Bottom;
        }
        if both {
            self.view_side = ViewSide::Both;
        }
        if names {
            self.show_names = !self.show_names;
        }
        if pins {
            self.show_pins = !self.show_pins;
        }
        if zoom_in {
            self.zoom_center(1.2);
        }
        if zoom_out {
            self.zoom_center(1.0 / 1.2);
        }
    }

    fn zoom_center(&mut self, factor: f32) {
        let rect = self.last_view_rect;
        if rect.width() < 10.0 {
            self.needs_fit = true;
            return;
        }
        let anchor = rect.center();
        let Some(board) = self.board.as_ref() else {
            return;
        };
        let before = self.screen_to_board(board, anchor);
        let view = self.to_view(board, before);
        self.camera.scale = (self.camera.scale * factor).clamp(0.001, 50.0);
        self.camera.offset = Vec2::new(
            anchor.x - view.x * self.camera.scale,
            anchor.y - view.y * self.camera.scale,
        );
    }

    fn fit_board(&mut self, board: &Board, rect: Rect) {
        let b = board.bounds;
        let w = b.width().max(1.0);
        let h = b.height().max(1.0);
        let scale = ((rect.width() - 56.0) / w)
            .min((rect.height() - 56.0) / h)
            .clamp(0.001, 50.0);
        let center = self.to_view(board, b.center());
        self.camera.scale = scale;
        self.camera.offset = Vec2::new(
            rect.center().x - center.x * scale,
            rect.center().y - center.y * scale,
        );
        self.needs_fit = false;
    }

    fn zoom_to_bbox(&mut self, bbox: BBox) {
        let rect = self.last_view_rect;
        if rect.width() < 10.0 || bbox.is_empty() {
            self.needs_fit = true;
            return;
        }
        let Some(board) = self.board.as_ref() else {
            return;
        };
        let span = bbox.width().max(bbox.height()).max(90.0);
        let padded = bbox.padded(span * 0.18);
        let scale = ((rect.width() - 100.0) / padded.width().max(1.0))
            .min((rect.height() - 100.0) / padded.height().max(1.0))
            .clamp(0.002, 20.0);
        let center = self.to_view(board, padded.center());
        self.camera.scale = scale;
        self.camera.offset = Vec2::new(
            rect.center().x - center.x * scale,
            rect.center().y - center.y * scale,
        );
        self.needs_fit = false;
    }

    fn focus_hit(&mut self, hit: Hit) {
        let mut target = None;
        let mut side = None;
        let mut part_sel = None;
        let mut pin_sel = None;
        let mut net_sel = None;

        if let Some(board) = self.board.as_ref() {
            match &hit {
                Hit::Part(i) => {
                    if let Some(part) = board.parts.get(*i) {
                        target = Some(part.bounds);
                        side = Some(part.side);
                        part_sel = Some(*i);
                    }
                }
                Hit::Pin(i) => {
                    if let Some(pin) = board.pins.get(*i) {
                        target = Some(BBox::from_point(pin.pos));
                        side = Some(pin.side);
                        part_sel = pin.part;
                        pin_sel = Some(*i);
                        if !pin.net.is_empty() {
                            net_sel = Some(pin.net.clone());
                        }
                    }
                }
                Hit::Net(net) => {
                    target = board.net_bounds(net);
                    net_sel = Some(net.clone());
                    if let Some(pin) = board.pins.iter().find(|p| &p.net == net) {
                        part_sel = pin.part;
                        side = pin.part.and_then(|i| board.parts.get(i)).map(|p| p.side);
                    }
                }
            }
        }

        self.selected_part = part_sel;
        self.selected_pin = pin_sel;
        self.selected_net = net_sel;
        if let Some(side) = side {
            match side {
                Side::Top => self.view_side = ViewSide::Top,
                Side::Bottom => self.view_side = ViewSide::Bottom,
                Side::Both => {}
            }
        }
        if let Some(bbox) = target {
            self.zoom_to_bbox(bbox);
        } else if matches!(hit, Hit::Net(_)) {
            self.needs_fit = true;
        }
    }

    fn to_view(&self, board: &Board, p: Pt) -> Pt {
        if self.view_side == ViewSide::Bottom {
            let axis = board.bounds.min.x + board.bounds.max.x;
            Pt::new(axis - p.x, p.y)
        } else {
            p
        }
    }

    fn to_screen(&self, board: &Board, p: Pt) -> Pos2 {
        let v = self.to_view(board, p);
        Pos2::new(
            self.camera.offset.x + v.x * self.camera.scale,
            self.camera.offset.y + v.y * self.camera.scale,
        )
    }

    fn screen_to_board(&self, board: &Board, pos: Pos2) -> Pt {
        let scale = self.camera.scale.max(0.000_001);
        let v = Pt::new(
            (pos.x - self.camera.offset.x) / scale,
            (pos.y - self.camera.offset.y) / scale,
        );
        if self.view_side == ViewSide::Bottom {
            let axis = board.bounds.min.x + board.bounds.max.x;
            Pt::new(axis - v.x, v.y)
        } else {
            v
        }
    }

    fn screen_rect(&self, board: &Board, bbox: BBox) -> Rect {
        Rect::from_two_pos(
            self.to_screen(board, bbox.min),
            self.to_screen(board, bbox.max),
        )
    }

    fn view_bounds(&self, board: &Board, rect: Rect) -> BBox {
        let mut b = BBox::from_two(
            self.screen_to_board(board, rect.min),
            self.screen_to_board(board, rect.max),
        );
        b = b.padded(8.0 / self.camera.scale.max(0.000_001));
        b
    }

    fn pick_pin(&self, board: &Board, screen: Pos2) -> Option<usize> {
        let mut best = None;
        let mut best_d = f32::INFINITY;
        let side = self.view_side.side();
        for (idx, pin) in board.pins.iter().enumerate() {
            if !board.visible_pin(pin, side) {
                continue;
            }
            let sp = self.to_screen(board, pin.pos);
            let d = sp.distance(screen);
            let radius = (pin.radius * self.camera.scale).clamp(2.5, 12.0) + 5.0;
            if d <= radius && d < best_d {
                best = Some(idx);
                best_d = d;
            }
        }
        best
    }

    fn pick_nail(&self, board: &Board, screen: Pos2) -> Option<usize> {
        let mut best = None;
        let mut best_d = f32::INFINITY;
        let side = self.view_side.side();
        for (idx, nail) in board.nails.iter().enumerate() {
            let visible = match side {
                Side::Both => true,
                Side::Top => nail.side != Side::Bottom,
                Side::Bottom => nail.side != Side::Top,
            };
            if !visible {
                continue;
            }
            let sp = self.to_screen(board, nail.pos);
            let d = sp.distance(screen);
            if d <= 8.0 && d < best_d {
                best = Some(idx);
                best_d = d;
            }
        }
        best
    }

    fn pick_part(&self, board: &Board, screen: Pos2) -> Option<usize> {
        let pos = self.screen_to_board(board, screen);
        let side = self.view_side.side();
        let mut best: Option<(usize, f32)> = None;
        for (idx, part) in board.parts.iter().enumerate() {
            if !board.visible_part(part, side) || part.bounds.is_empty() {
                continue;
            }
            if part.bounds.contains(pos) {
                let area = part.bounds.area();
                if best.map(|(_, a)| area < a).unwrap_or(true) {
                    best = Some((idx, area));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    fn draw_board(&self, board: &Board, painter: &egui::Painter, rect: Rect) {
        let view = self.view_bounds(board, rect);
        let scale = self.camera.scale;
        let side = self.view_side.side();

        if board.outline.len() >= 2 {
            let pts: Vec<Pos2> = board
                .outline
                .iter()
                .map(|p| self.to_screen(board, *p))
                .collect();
            if pts.len() >= 3 {
                painter.add(Shape::convex_polygon(
                    pts.clone(),
                    BOARD_FILL,
                    Stroke::new(1.4, OUTLINE),
                ));
            }
            painter.add(Shape::closed_line(pts, Stroke::new(1.4, OUTLINE)));
        } else if let Some(limits) = board.limits
            && !limits.is_empty()
        {
            let r = self.screen_rect(board, limits);
            painter.rect_filled(r, CornerRadius::ZERO, BOARD_FILL);
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(1.4, OUTLINE),
                StrokeKind::Inside,
            );
        }

        for (a, b) in &board.segments {
            painter.line_segment(
                [self.to_screen(board, *a), self.to_screen(board, *b)],
                Stroke::new(1.2, OUTLINE),
            );
        }
        for (a, b) in &board.overlay {
            painter.line_segment(
                [self.to_screen(board, *a), self.to_screen(board, *b)],
                Stroke::new(1.0, Color32::from_rgb(76, 99, 78)),
            );
        }

        for (idx, part) in board.parts.iter().enumerate() {
            if !board.visible_part(part, side) || part.bounds.is_empty() {
                continue;
            }
            if !part.bounds.intersects(view) {
                continue;
            }
            let r = self.screen_rect(board, part.bounds);
            if r.width() < 1.0 || r.height() < 1.0 {
                continue;
            }
            let (fill, stroke) = match part.side {
                Side::Top => (PART_TOP, PART_TOP_STROKE),
                Side::Bottom => (PART_BOTTOM, PART_BOTTOM_STROKE),
                Side::Both => (PART_BOTH, PART_BOTH_STROKE),
            };
            painter.rect_filled(r, CornerRadius::ZERO, fill);
            let selected = self.selected_part == Some(idx);
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected { PART_SELECT } else { stroke },
                ),
                StrokeKind::Inside,
            );

            if self.show_names && r.width() > 20.0 && r.height() > 8.0 && scale > 0.035 {
                let font_size = (9.5 + scale * 1.4).clamp(9.5, 17.0);
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    &part.name,
                    FontId::proportional(font_size),
                    if selected { Color32::WHITE } else { TEXT },
                );
            }
        }

        let selected_net = self.selected_net.as_deref();
        for nail in &board.nails {
            if !board.visible_pin(&Pin::new(nail.pos), side) {
                continue;
            }
            if !view.contains(nail.pos) {
                continue;
            }
            let sp = self.to_screen(board, nail.pos);
            let highlighted = selected_net
                .map(|net| !net.is_empty() && nail.net == net)
                .unwrap_or(false);
            let color = if highlighted {
                NET_HIGHLIGHT
            } else {
                pin_color(&nail.net, nail.side)
            };
            let size = if highlighted { 7.0 } else { 5.0 };
            let r = Rect::from_center_size(sp, Vec2::splat(size));
            painter.rect_filled(r, CornerRadius::ZERO, color);
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(0.9, Color32::from_rgb(12, 15, 20)),
                StrokeKind::Inside,
            );
        }

        if self.show_pins {
            for pass in 0..2 {
                for (idx, pin) in board.pins.iter().enumerate() {
                    if !board.visible_pin(pin, side) {
                        continue;
                    }
                    let highlighted = selected_net
                        .map(|net| !net.is_empty() && pin.net == net)
                        .unwrap_or(false);
                    if (pass == 0) == highlighted {
                        continue;
                    }
                    if !view.contains(pin.pos) {
                        continue;
                    }
                    let sp = self.to_screen(board, pin.pos);
                    let mut radius = (pin.radius * scale).clamp(2.3, 10.0);
                    if highlighted || self.selected_pin == Some(idx) {
                        radius += 1.2;
                    }
                    let color = if highlighted && self.selected_pin != Some(idx) {
                        NET_HIGHLIGHT
                    } else {
                        pin_color(&pin.net, pin.side)
                    };
                    painter.circle_filled(sp, radius, color);
                    painter.circle_stroke(
                        sp,
                        radius,
                        Stroke::new(0.8, Color32::from_rgb(18, 22, 28)),
                    );
                    if self.selected_pin == Some(idx) {
                        painter.circle_stroke(sp, radius + 3.0, Stroke::new(2.0, Color32::WHITE));
                    } else if self.hovered_pin == Some(idx) {
                        painter.circle_stroke(
                            sp,
                            radius + 2.5,
                            Stroke::new(1.6, Color32::from_rgb(255, 194, 86)),
                        );
                    }
                }
            }
        }
    }

    fn view(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;
        self.last_view_rect = rect;
        painter.rect_filled(rect, CornerRadius::ZERO, BG);

        let Some(mut board) = self.board.take() else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Arraste um arquivo de boardview ou clique em Abrir",
                FontId::proportional(21.0),
                MUTED,
            );
            painter.text(
                rect.center() + Vec2::new(0.0, 30.0),
                Align2::CENTER_CENTER,
                "BRD · BRD2 · BVR · BVR2 · BVR3 · BDV · FBV · FZ",
                FontId::proportional(14.0),
                Color32::from_rgb(86, 96, 108),
            );
            return;
        };

        if self.needs_fit {
            self.fit_board(&board, rect);
        }
        if let Some(hit) = self.pending_focus.take() {
            self.board = Some(board);
            self.focus_hit(hit);
            board = self.board.take().expect("board restaurado");
        }

        if response.dragged_by(egui::PointerButton::Primary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.camera.offset += delta;
        }

        let pointer = ui.input(|i| i.pointer.hover_pos());
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let pinch = ui.input(|i| i.zoom_delta());
            let factor = (scroll * 0.0022).exp() * pinch;
            if (factor - 1.0).abs() > 0.0001
                && let Some(anchor) = pointer
            {
                let before = self.screen_to_board(&board, anchor);
                let view = self.to_view(&board, before);
                self.camera.scale = (self.camera.scale * factor).clamp(0.001, 50.0);
                self.camera.offset = Vec2::new(
                    anchor.x - view.x * self.camera.scale,
                    anchor.y - view.y * self.camera.scale,
                );
            }
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        let hit_pin = pointer.and_then(|p| self.pick_pin(&board, p));
        self.hovered_pin = hit_pin;

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            if let Some(idx) = self.pick_pin(&board, pos) {
                self.selected_pin = Some(idx);
                self.selected_part = board.pins[idx].part;
                self.selected_net = if board.pins[idx].net.is_empty() {
                    None
                } else {
                    Some(board.pins[idx].net.clone())
                };
            } else if let Some(idx) = self.pick_nail(&board, pos) {
                self.selected_part = None;
                self.selected_pin = None;
                self.selected_net = if board.nails[idx].net.is_empty() {
                    None
                } else {
                    Some(board.nails[idx].net.clone())
                };
            } else if let Some(idx) = self.pick_part(&board, pos) {
                self.selected_part = Some(idx);
                self.selected_pin = None;
                self.selected_net = None;
            } else {
                self.selected_part = None;
                self.selected_pin = None;
                self.selected_net = None;
            }
        }

        if response.double_clicked() {
            let mut zoom_target: Option<BBox> = None;
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(idx) = self.pick_pin(&board, pos) {
                    let pin = &board.pins[idx];
                    zoom_target = Some(
                        pin.part
                            .and_then(|i| board.parts.get(i))
                            .map(|p| p.bounds)
                            .unwrap_or_else(|| BBox::from_point(pin.pos)),
                    );
                } else if let Some(idx) = self.pick_nail(&board, pos) {
                    let nail = &board.nails[idx];
                    zoom_target = board
                        .net_bounds(&nail.net)
                        .or_else(|| Some(BBox::from_point(nail.pos)));
                } else if let Some(idx) = self.pick_part(&board, pos) {
                    zoom_target = Some(board.parts[idx].bounds);
                }
            }
            if let Some(bbox) = zoom_target {
                self.board = Some(board);
                self.zoom_to_bbox(bbox);
                board = self.board.take().expect("board restaurado");
            }
        }

        self.draw_board(&board, &painter, rect);

        if let (Some(idx), Some(pos)) = (self.hovered_pin, pointer)
            && let Some(pin) = board.pins.get(idx)
        {
            let part = pin.part.map(|i| board.part_name(i)).unwrap_or("(avulso)");
            let net = if pin.net.is_empty() {
                "—"
            } else {
                pin.net.as_str()
            };
            let text = format!("{part} · pino {}  |  net: {net}", pin.label());
            let font = FontId::proportional(13.0);
            let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
            let mut p = pos + Vec2::new(16.0, 16.0);
            let size = galley.size() + Vec2::new(12.0, 8.0);
            p.x = p.x.min(rect.right() - size.x - 4.0).max(rect.left() + 4.0);
            p.y = p.y.min(rect.bottom() - size.y - 4.0).max(rect.top() + 4.0);
            let bg = Rect::from_min_size(p, size);
            painter.rect_filled(bg, CornerRadius::ZERO, Color32::from_black_alpha(235));
            painter.rect_stroke(
                bg,
                CornerRadius::ZERO,
                Stroke::new(1.0, Color32::from_rgb(88, 100, 116)),
                StrokeKind::Inside,
            );
            painter.galley(p + Vec2::new(6.0, 4.0), galley, Color32::WHITE);
        }

        self.board = Some(board);
    }

    fn fz_key_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_fz_key_dialog {
            return;
        }

        let mut open = true;
        let mut apply = false;
        let mut load_file = false;
        let mut close = false;
        let retry_label = self
            .retry_path
            .as_ref()
            .map(|path| format!("Reabrir: {}", path.display()))
            .unwrap_or_default();
        let key_error = self.fz_key_error.clone();

        egui::Window::new("Chave FZ (RC6)")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.label(
                    "Arquivos .fz cifrados exigem a chave RC6 de 44 palavras de 32 bits. \
                     Cole a chave completa ou carregue um arquivo com a linha FZKey.",
                );
                ui.label(
                    egui::RichText::new(
                        "O BoardEX não inclui nenhuma chave. Use somente uma chave que você \
                         possui e tem direito de usar.",
                    )
                    .color(MUTED)
                    .small(),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.fz_key_input)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("0x12345678, 0x9abcdef0, ... (44 valores)"),
                );
                if !retry_label.is_empty() {
                    ui.label(egui::RichText::new(retry_label).color(MUTED).small());
                }
                if let Some(err) = &key_error {
                    ui.colored_label(Color32::from_rgb(235, 105, 95), err);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Aplicar e reabrir").clicked() {
                        apply = true;
                    }
                    if ui.button("Carregar chave…").clicked() {
                        load_file = true;
                    }
                    if ui.button("Fechar").clicked() {
                        close = true;
                    }
                });
            });

        if load_file
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Chave/configuração", &["conf", "txt", "ini", "*"])
                .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    self.fz_key_input = text;
                    self.fz_key_error = None;
                }
                Err(err) => {
                    self.fz_key_error = Some(format!("{}: {err}", path.display()));
                }
            }
        }

        if close {
            open = false;
        }
        if apply {
            match formats::fz::set_key(&self.fz_key_input) {
                Ok(()) => {
                    self.fz_key_error = None;
                    if let Some(path) = self.retry_path.clone() {
                        self.open_path(path);
                    } else {
                        self.status = "Chave FZ definida.".to_string();
                        self.show_fz_key_dialog = false;
                    }
                }
                Err(err) => {
                    self.fz_key_error = Some(err);
                    self.show_fz_key_dialog = true;
                }
            }
        } else {
            self.show_fz_key_dialog = open;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Abrir…").clicked() {
                self.open_dialog();
            }
            if ui
                .button("Chave FZ…")
                .on_hover_text("Chave RC6 para arquivos .fz criptografados")
                .clicked()
            {
                self.show_fz_key_dialog = true;
            }
            if ui.button("Ajustar").on_hover_text("F / Home").clicked() {
                self.needs_fit = true;
            }
            ui.separator();
            ui.selectable_value(&mut self.view_side, ViewSide::Top, "Topo");
            ui.selectable_value(&mut self.view_side, ViewSide::Bottom, "Fundo");
            ui.selectable_value(&mut self.view_side, ViewSide::Both, "Ambos");
            ui.separator();
            ui.checkbox(&mut self.show_names, "Nomes");
            ui.checkbox(&mut self.show_pins, "Pinos");
            ui.checkbox(&mut self.show_details, "Detalhes");
            ui.separator();

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Buscar componente, pino ou net…")
                    .desired_width(280.0),
            );
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            if response.changed() {
                self.update_search();
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            if enter
                && (response.has_focus() || response.lost_focus())
                && let Some(row) = self.hits.first().cloned()
            {
                self.pending_focus = Some(row.hit);
            }

            if let Some(board) = self.board.as_ref() {
                ui.label(
                    egui::RichText::new(format!("{:.2} px/mil", self.camera.scale))
                        .color(MUTED)
                        .small(),
                );
                let _ = &board;
            }
        });
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<Hit> = None;
        let mut go_to: Option<BBox> = None;
        let mut highlight_net: Option<String> = None;

        {
            let Some(board) = self.board.as_ref() else {
                ui.heading("BoardEX");
                ui.label("Nenhuma placa carregada.");
                ui.add_space(8.0);
                ui.label("Formatos: BRD, BRD2, BVR, BVR2, BVR3, BDV, FBV, FZ.");
                ui.label(
                    "Atalhos: F ajustar · T topo · B fundo · A ambos · Esc limpar · Ctrl+F buscar.",
                );
                return;
            };

            ui.heading("BoardEX");
            ui.label(
                egui::RichText::new(format!("{}  ·  {}", board.format.label(), board.name))
                    .color(ACCENT),
            );
            ui.label(format!(
                "{} componentes · {} pinos · {} nails · {} nets",
                board.parts.len(),
                board.pins.len(),
                board.nails.len(),
                board.nets.len()
            ));
            ui.label(format!(
                "Área: {:.0} × {:.0} mil",
                board.bounds.width(),
                board.bounds.height()
            ));
            if let Some(path) = &self.board_path {
                ui.label(
                    egui::RichText::new(path.display().to_string())
                        .color(MUTED)
                        .small(),
                );
            }

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Tensões:").color(MUTED).small());
                for rail in VOLTAGE_RAILS {
                    ui.label(egui::RichText::new("●").size(14.0).color(rail.color()));
                    ui.label(egui::RichText::new(rail.label()).color(MUTED).small());
                    ui.add_space(2.0);
                }
            });
            ui.add_space(4.0);

            if !board.warnings.is_empty() {
                ui.collapsing(format!("Avisos ({})", board.warnings.len()), |ui| {
                    for warning in &board.warnings {
                        ui.label(warning);
                    }
                });
            }

            if !self.query.trim().is_empty() {
                ui.separator();
                ui.heading(format!("Busca ({})", self.hits.len()));
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if self.hits.is_empty() {
                            ui.label("Nada encontrado.");
                        }
                        for row in &self.hits {
                            let response = ui.selectable_label(false, &row.label);
                            if response.clicked() {
                                clicked = Some(row.hit.clone());
                            }
                            response.on_hover_text(&row.detail);
                        }
                    });
            }

            if let Some(idx) = self.selected_part
                && let Some(part) = board.parts.get(idx)
            {
                ui.separator();
                ui.heading("Componente selecionado");
                ui.label(egui::RichText::new(&part.name).strong().size(17.0));
                ui.label(
                    egui::RichText::new(format!(
                        "Tipo: {}",
                        classify_component(&part.name).label()
                    ))
                    .strong()
                    .color(ACCENT),
                );
                ui.label(format!(
                    "Montagem: {} · lado {} · {} pino(s)",
                    part.kind.label(),
                    part.side.label(),
                    part.pins.len()
                ));
                if part.has_rect() {
                    let width = (part.p2.x - part.p1.x).abs();
                    let height = (part.p2.y - part.p1.y).abs();
                    ui.label(format!("Dimensões: {width:.1} × {height:.1} mil"));
                }
                if !part.value.is_empty() {
                    ui.label(format!("Valor: {}", part.value));
                }
                if !part.mfgcode.is_empty() {
                    ui.label(format!("Código do fabricante: {}", part.mfgcode));
                }

                let mut nets: Vec<&str> = part
                    .pins
                    .iter()
                    .filter_map(|pin_idx| board.pins.get(*pin_idx))
                    .map(|pin| pin.net.as_str())
                    .filter(|net| !net.is_empty())
                    .collect();
                nets.sort_unstable();
                nets.dedup();
                if !nets.is_empty() {
                    let preview = nets.iter().take(6).copied().collect::<Vec<_>>().join(", ");
                    let text = if nets.len() > 6 {
                        format!("Nets ({}): {preview}, …", nets.len())
                    } else {
                        format!("Nets ({}): {preview}", nets.len())
                    };
                    ui.label(text).on_hover_text(nets.join("\n"));
                }

                if ui.button("Enquadrar componente").clicked() {
                    go_to = Some(part.bounds);
                }
            }

            if let Some(idx) = self.selected_pin
                && let Some(pin) = board.pins.get(idx)
            {
                ui.separator();
                ui.heading("Pino");
                let part = pin.part.map(|i| board.part_name(i)).unwrap_or("(avulso)");
                ui.label(egui::RichText::new(format!("{part} · {}", pin.label())).strong());
                ui.label(format!(
                    "net: {}",
                    if pin.net.is_empty() { "—" } else { &pin.net }
                ));
                ui.label(format!(
                    "lado {} · posição {:.1}, {:.1} mil",
                    pin.side.label(),
                    pin.pos.x,
                    pin.pos.y
                ));
                if !pin.net.is_empty() && ui.button("Destacar net").clicked() {
                    highlight_net = Some(pin.net.clone());
                }
                if ui.button("Enquadrar pino").clicked() {
                    go_to = Some(BBox::from_point(pin.pos));
                }
            }

            if let Some(net) = &self.selected_net {
                let bounds = board.net_bounds(net);
                let pin_count = board.pins.iter().filter(|p| &p.net == net).count();
                let nail_count = board.nails.iter().filter(|n| &n.net == net).count();
                ui.separator();
                ui.heading("Net destacada");
                ui.label(egui::RichText::new(net).color(NET_HIGHLIGHT).strong());
                let mut counts = format!("{pin_count} pino(s)");
                if nail_count > 0 {
                    counts.push_str(&format!(" · {nail_count} nail(s)"));
                }
                ui.label(counts);
                if let Some(bounds) = bounds
                    && ui.button("Enquadrar net").clicked()
                {
                    go_to = Some(bounds);
                }
            }

            ui.separator();
            ui.label(
                egui::RichText::new(
                    "Clique num pino ou nail para destacar a net. Clique num componente para selecioná-lo.\n\
                     Arraste para mover · roda do mouse para zoom · duplo clique enquadra.",
                )
                .color(MUTED)
                .small(),
            );
        }

        if let Some(hit) = clicked {
            self.focus_hit(hit);
        }
        if let Some(net) = highlight_net {
            self.selected_net = Some(net);
        }
        if let Some(bbox) = go_to {
            self.zoom_to_bbox(bbox);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let error = self.error.clone();
            if let Some(error) = error {
                ui.colored_label(Color32::from_rgb(255, 120, 112), &error);
                if ui.small_button("fechar").clicked() {
                    self.error = None;
                }
            } else if let Some(board) = self.board.as_ref() {
                ui.label(format!(
                    "{} · {} componentes · {} pinos · {} nails · {} nets",
                    board.format.label(),
                    board.parts.len(),
                    board.pins.len(),
                    board.nails.len(),
                    board.nets.len()
                ));
                if !board.warnings.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("({} avisos)", board.warnings.len()))
                            .color(Color32::from_rgb(230, 180, 90)),
                    );
                }
            } else {
                ui.label(&self.status);
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
                if let (Some(pos), Some(board)) = (pointer, self.board.as_ref())
                    && self.last_view_rect.contains(pos)
                {
                    let p = self.screen_to_board(board, pos);
                    let mut text = format!("{:.0}, {:.0} mil", p.x, p.y);
                    if let Some(idx) = self.hovered_pin
                        && let Some(pin) = board.pins.get(idx)
                    {
                        let part = pin.part.map(|i| board.part_name(i)).unwrap_or("(avulso)");
                        let net = if pin.net.is_empty() { "—" } else { &pin.net };
                        text = format!("{part} · {} · net {net}  |  {text}", pin.label());
                    }
                    ui.label(egui::RichText::new(text).color(MUTED).small());
                }
            });
        });
    }
}

impl eframe::App for BoardExApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_keys(ui.ctx());

        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped.first() {
            let path = file.path().to_path_buf();
            if self.board_path.as_ref() != Some(&path) {
                self.open_path(path);
            }
        }

        egui::Panel::top("boardex_toolbar").show(ui, |ui| {
            self.toolbar(ui);
        });

        if self.show_details {
            egui::Panel::right("boardex_details")
                .resizable(true)
                .default_size(325.0)
                .min_size(230.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.details(ui);
                    });
                });
        }

        egui::Panel::bottom("boardex_status").show(ui, |ui| {
            self.status_bar(ui);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.view(ui);
        });

        self.fz_key_dialog(ui.ctx());
    }
}

fn configure_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(19, 24, 30);
    visuals.window_fill = Color32::from_rgb(24, 30, 37);
    visuals.extreme_bg_color = Color32::from_rgb(12, 16, 21);
    visuals.faint_bg_color = Color32::from_rgb(28, 35, 43);
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = Color32::from_rgb(43, 103, 158);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_voltage_rails() {
        let cases = [
            ("+12V", Some(VoltageRail::V12to20)),
            ("12V0", Some(VoltageRail::V12to20)),
            ("+12V5", Some(VoltageRail::V12to20)),
            ("+15V", Some(VoltageRail::V12to20)),
            ("+19.5V", Some(VoltageRail::V12to20)),
            ("+20V", Some(VoltageRail::V12to20)),
            ("VCC12V0", Some(VoltageRail::V12to20)),
            ("+5V", Some(VoltageRail::V5)),
            ("5V0", Some(VoltageRail::V5)),
            ("VCC5V0", Some(VoltageRail::V5)),
            ("PP5V_USB", Some(VoltageRail::V5)),
            ("+3V3", Some(VoltageRail::V3V3)),
            ("3.3V", Some(VoltageRail::V3V3)),
            ("3,3V", Some(VoltageRail::V3V3)),
            ("VCC3V3", Some(VoltageRail::V3V3)),
            ("PP3V3_S5", Some(VoltageRail::V3V3)),
            ("+1V8", Some(VoltageRail::V1V8)),
            ("1.8V", Some(VoltageRail::V1V8)),
            ("VCC1V8", Some(VoltageRail::V1V8)),
            ("+1V8_S0", Some(VoltageRail::V1V8)),
            ("+1V", Some(VoltageRail::V1)),
            ("1V0", Some(VoltageRail::V1)),
            ("1.0V", Some(VoltageRail::V1)),
            ("1V05", Some(VoltageRail::V1)),
            ("PP1V0", Some(VoltageRail::V1)),
            ("GND", None),
            ("+1V2", None),
            ("+2V5", None),
            ("+5V5", None),
            ("+11V", None),
            ("+21V", None),
        ];

        for (net, expected) in cases {
            assert_eq!(classify_net_voltage(net), expected, "net: {net}");
        }
    }

    #[test]
    fn falls_back_to_pin_side_color() {
        assert_eq!(pin_color("+3V3", Side::Top), VOLT_3V3);
        assert_eq!(pin_color("+1V8", Side::Bottom), VOLT_1V8);
        assert_eq!(pin_color("GND", Side::Top), PIN_TOP);
        assert_eq!(pin_color("", Side::Bottom), PIN_BOTTOM);
        assert_eq!(pin_color("SIGNAL", Side::Both), PIN_BOTH);
    }

    #[test]
    fn classifies_component_types_by_designator() {
        let cases = [
            ("C1", ComponentType::Capacitor),
            ("c12", ComponentType::Capacitor),
            ("CAP2", ComponentType::Capacitor),
            ("CAPACITOR1", ComponentType::Capacitor),
            ("R1", ComponentType::Resistor),
            ("#R5", ComponentType::Resistor),
            ("RN1", ComponentType::Resistor),
            ("RESISTOR1", ComponentType::Resistor),
            ("L1", ComponentType::Inductor),
            ("FB1", ComponentType::Inductor),
            ("BOBINA1", ComponentType::Inductor),
            ("Q1", ComponentType::TransistorMosfet),
            ("T1", ComponentType::Transistor),
            ("TR2", ComponentType::Transistor),
            ("TRANSISTOR1", ComponentType::Transistor),
            ("MOS1", ComponentType::Mosfet),
            ("FET2", ComponentType::Mosfet),
            ("MOSFET1", ComponentType::Mosfet),
            ("D1", ComponentType::Diode),
            ("DIODO1", ComponentType::Diode),
            ("LED1", ComponentType::Led),
            ("U1", ComponentType::Ic),
            ("IC1", ComponentType::Ic),
            ("CI1", ComponentType::Ic),
            ("CHIP1", ComponentType::Ic),
            ("Y1", ComponentType::CrystalOscillator),
            ("X1", ComponentType::CrystalOscillator),
            ("OSC1", ComponentType::CrystalOscillator),
            ("CRISTAL1", ComponentType::CrystalOscillator),
            ("F1", ComponentType::Fuse),
            ("FUSIVEL1", ComponentType::Fuse),
            ("BT1", ComponentType::Battery),
            ("BATERIA1", ComponentType::Battery),
            ("J1", ComponentType::Connector),
            ("P1", ComponentType::Connector),
            ("CONECTOR1", ComponentType::Connector),
            ("SW1", ComponentType::Switch),
            ("S1", ComponentType::Switch),
            ("BOTAO1", ComponentType::Switch),
            ("JP1", ComponentType::Jumper),
            ("K1", ComponentType::Relay),
            ("RELE1", ComponentType::Relay),
            ("LS1", ComponentType::Speaker),
            ("BUZZER1", ComponentType::Speaker),
            ("ANT1", ComponentType::Antenna),
            ("TP1", ComponentType::TestPoint),
            ("M1", ComponentType::Motor),
            ("UNKNOWN", ComponentType::Other),
        ];

        for (name, expected) in cases {
            assert_eq!(classify_component(name), expected, "componente: {name}");
        }
    }
}
