//! Modelo de dados independente de formato.
//!
//! Unidades internas: "mil" (milésimo de polegada). O eixo Y cresce para baixo,
//! como na maioria dos arquivos de boardview.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: Pt,
    pub max: Pt,
}

impl BBox {
    pub fn empty() -> Self {
        Self {
            min: Pt::new(f32::INFINITY, f32::INFINITY),
            max: Pt::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
        }
    }

    pub fn from_point(p: Pt) -> Self {
        Self { min: p, max: p }
    }

    pub fn from_two(a: Pt, b: Pt) -> Self {
        Self {
            min: Pt::new(a.x.min(b.x), a.y.min(b.y)),
            max: Pt::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    pub fn is_empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn add(&mut self, p: Pt) {
        if !p.is_finite() {
            return;
        }
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn union(&mut self, other: Self) {
        if other.is_empty() {
            return;
        }
        self.add(other.min);
        self.add(other.max);
    }

    pub fn width(self) -> f32 {
        (self.max.x - self.min.x).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.max.y - self.min.y).max(0.0)
    }

    pub fn center(self) -> Pt {
        Pt::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    pub fn padded(self, margin: f32) -> Self {
        if self.is_empty() {
            return self;
        }
        Self {
            min: Pt::new(self.min.x - margin, self.min.y - margin),
            max: Pt::new(self.max.x + margin, self.max.y + margin),
        }
    }

    pub fn contains(self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn intersects(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    pub fn area(self) -> f32 {
        self.width() * self.height()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Both,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "topo",
            Self::Bottom => "fundo",
            Self::Both => "ambos",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Smd,
    ThroughHole,
}

impl PartKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smd => "SMD",
            Self::ThroughHole => "through-hole",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub value: String,
    pub mfgcode: String,
    pub side: Side,
    pub kind: PartKind,
    /// Canto do retângulo do componente (quando o formato fornece).
    pub p1: Pt,
    pub p2: Pt,
    /// Limites calculados a partir do retângulo e dos pinos.
    pub bounds: BBox,
    /// Índices de pinos pertencentes a este componente.
    pub pins: Vec<usize>,
}

impl Part {
    pub fn new(name: String) -> Self {
        Self {
            name,
            value: String::new(),
            mfgcode: String::new(),
            side: Side::Both,
            kind: PartKind::Smd,
            p1: Pt::ZERO,
            p2: Pt::ZERO,
            bounds: BBox::empty(),
            pins: Vec::new(),
        }
    }

    pub fn has_rect(&self) -> bool {
        self.p1 != self.p2 && self.p1.is_finite() && self.p2.is_finite()
    }
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub pos: Pt,
    pub part: Option<usize>,
    pub number: String,
    pub name: String,
    pub net: String,
    pub side: Side,
    /// Raio em mils. O renderizador aplica um mínimo em pixels.
    pub radius: f32,
    pub probe: i32,
}

impl Pin {
    pub fn new(pos: Pt) -> Self {
        Self {
            pos,
            part: None,
            number: String::new(),
            name: String::new(),
            net: String::new(),
            side: Side::Both,
            radius: 6.0,
            probe: 0,
        }
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if !self.number.is_empty() {
            s.push_str(&self.number);
        }
        if !self.name.is_empty() {
            if !s.is_empty() {
                s.push_str(" / ");
            }
            s.push_str(&self.name);
        }
        if s.is_empty() {
            s.push('—');
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct Nail {
    pub pos: Pt,
    pub side: Side,
    pub net: String,
    /// Número do probe/test point, quando o formato informa.
    #[allow(dead_code)]
    pub probe: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    Brd,
    Brd2,
    Bvr,
    Bvr2,
    Bvr3,
    Bdv,
    Fbv,
    Fz,
}

impl FileFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Brd => "BRD",
            Self::Brd2 => "BRD2",
            Self::Bvr => "BVR",
            Self::Bvr2 => "BVR2",
            Self::Bvr3 => "BVR3",
            Self::Bdv => "BDV",
            Self::Fbv => "FBV",
            Self::Fz => "FZ",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Brd => "brd",
            Self::Brd2 => "brd2",
            Self::Bvr => "bvr",
            Self::Bvr2 => "bvr2",
            Self::Bvr3 => "bvr3",
            Self::Bdv => "bdv",
            Self::Fbv => "fbv",
            Self::Fz => "fz",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Board {
    pub name: String,
    pub format: FileFormat,
    /// Contorno principal (polígono fechado, quando conhecido).
    pub outline: Vec<Pt>,
    /// Segmentos de contorno avulsos.
    pub segments: Vec<(Pt, Pt)>,
    /// Linhas de overlay/decoração.
    pub overlay: Vec<(Pt, Pt)>,
    pub parts: Vec<Part>,
    pub pins: Vec<Pin>,
    pub nails: Vec<Nail>,
    /// Limites físicos informados pelo cabeçalho, quando o formato os fornece.
    pub limits: Option<BBox>,
    pub bounds: BBox,
    pub nets: Vec<String>,
    pub warnings: Vec<String>,
}

impl Board {
    pub fn new(name: impl Into<String>, format: FileFormat) -> Self {
        Self {
            name: name.into(),
            format,
            outline: Vec::new(),
            segments: Vec::new(),
            overlay: Vec::new(),
            parts: Vec::new(),
            pins: Vec::new(),
            nails: Vec::new(),
            limits: None,
            bounds: BBox::empty(),
            nets: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Consolida limites, associa pinos a componentes e monta o índice de nets.
    pub fn finalize(&mut self) {
        for part in &mut self.parts {
            part.pins.clear();
            part.bounds = BBox::empty();
            if part.has_rect() {
                part.bounds.add(part.p1);
                part.bounds.add(part.p2);
            }
        }

        for (idx, pin) in self.pins.iter_mut().enumerate() {
            if !pin.radius.is_finite() || pin.radius <= 0.0 {
                pin.radius = 6.0;
            }
            if let Some(pi) = pin.part {
                if pi < self.parts.len() {
                    self.parts[pi].pins.push(idx);
                    self.parts[pi].bounds.add(pin.pos);
                } else {
                    pin.part = None;
                }
            }
        }

        for part in &mut self.parts {
            if part.bounds.is_empty() {
                if part.has_rect() {
                    part.bounds = BBox::from_two(part.p1, part.p2);
                } else {
                    continue;
                }
            }
            part.bounds = part.bounds.padded(10.0);
        }

        let mut b = BBox::empty();
        if let Some(limits) = self.limits {
            b.union(limits);
        }
        for p in &self.outline {
            b.add(*p);
        }
        for (a, c) in &self.segments {
            b.add(*a);
            b.add(*c);
        }
        for (a, c) in &self.overlay {
            b.add(*a);
            b.add(*c);
        }
        for part in &self.parts {
            if !part.bounds.is_empty() {
                b.union(part.bounds);
            }
        }
        for pin in &self.pins {
            b.add(pin.pos);
        }
        for nail in &self.nails {
            b.add(nail.pos);
        }

        if b.is_empty() {
            b = BBox::from_two(Pt::new(0.0, 0.0), Pt::new(1000.0, 800.0));
        }
        let pad = (b.width().max(b.height()) * 0.015).max(10.0);
        self.bounds = b.padded(pad);

        let mut nets: HashSet<&str> = HashSet::new();
        for pin in &self.pins {
            let net = pin.net.trim();
            if !net.is_empty() {
                nets.insert(net);
            }
        }
        for nail in &self.nails {
            let net = nail.net.trim();
            if !net.is_empty() {
                nets.insert(net);
            }
        }
        let mut nets: Vec<String> = nets.into_iter().map(str::to_owned).collect();
        nets.sort_by_key(|s| s.to_ascii_lowercase());
        self.nets = nets;
    }

    pub fn net_bounds(&self, net: &str) -> Option<BBox> {
        let mut b = BBox::empty();
        for pin in &self.pins {
            if pin.net == net {
                b.add(pin.pos);
            }
        }
        for nail in &self.nails {
            if nail.net == net {
                b.add(nail.pos);
            }
        }
        if b.is_empty() { None } else { Some(b) }
    }

    pub fn part_name(&self, idx: usize) -> &str {
        self.parts.get(idx).map(|p| p.name.as_str()).unwrap_or("?")
    }

    pub fn visible_pin(&self, pin: &Pin, view: Side) -> bool {
        match view {
            Side::Both => true,
            Side::Top => pin.side != Side::Bottom,
            Side::Bottom => pin.side != Side::Top,
        }
    }

    pub fn visible_part(&self, part: &Part, view: Side) -> bool {
        match view {
            Side::Both => true,
            Side::Top => part.side != Side::Bottom,
            Side::Bottom => part.side != Side::Top,
        }
    }
}
