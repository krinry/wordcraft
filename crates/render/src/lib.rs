//! WordCraft rasteriser: draws a page's display list with vello_cpu.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vello_cpu::kurbo::{self, Affine, BezPath, Shape};
use vello_cpu::peniko;
use vello_cpu::{Pixmap, RenderContext, Resources};
use wordcraft_doc::Document;
use wordcraft_doc::para::ShapeKind;
use wordcraft_doc::props::Rgb;
use wordcraft_fonts::FontDb;
use wordcraft_layout::Page;
use wordcraft_layout::display::{DisplayOptions, Draw, Stroke, page_display};

/// Worker threads for rasterising (0 on the web, where there are no threads).
pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map(|n| (n.get().saturating_sub(1)).clamp(1, 8) as u16).unwrap_or(2)
    }
}

/// Largest raster side vello_cpu handles comfortably.
pub const MAX_SIDE: u32 = 16_000;

/// A rendered image (premultiplied RGBA8, row-major).
#[derive(Clone, Default)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rendered {
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in px.iter_mut().take(3) {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// PNG bytes (empty on failure, logged).
    pub fn to_png(&self) -> Vec<u8> {
        let mut px = self.to_straight();
        px.resize(self.width as usize * self.height as usize * 4, 0);
        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, px) else {
            log::error!("PNG encode: bad size {}×{}", self.width, self.height);
            return Vec::new();
        };
        let mut buf = Vec::new();
        if let Err(e) = img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png) {
            log::error!("PNG encode: {e}");
            return Vec::new();
        }
        buf
    }
    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        let rgba = self.to_straight();
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            })
            .collect();
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        let _ = image::ImageEncoder::write_image(enc, &rgb, self.width, self.height, image::ExtendedColorType::Rgb8);
        buf
    }
    /// Straight RGBA at (x, y) (transparent outside).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y as usize * self.width as usize) + x as usize) * 4;
        let Some(p) = self.pixels.get(i..i + 4) else { return [0; 4] };
        let a = p[3] as u32;
        if a == 0 {
            return [0; 4];
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        [un(p[0]), un(p[1]), un(p[2]), p[3]]
    }
}

fn color(c: Rgb, alpha: f32) -> peniko::Color {
    peniko::Color::from_rgba8(c.0, c.1, c.2, (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Decoded images by media key (and content pointer), shared across renders.
fn image_cache() -> &'static Mutex<HashMap<(String, usize), Option<Arc<Pixmap>>>> {
    static C: std::sync::OnceLock<Mutex<HashMap<(String, usize), Option<Arc<Pixmap>>>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Decode an encoded image to a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = img
        .pixels()
        .map(|p| {
            let a = p[3] as u16;
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    Some(Pixmap::from_parts(data, w as u16, h as u16))
}

/// Pixel size of an encoded image.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

fn pixmap_for(doc: &Document, key: &str) -> Option<Arc<Pixmap>> {
    let bytes = doc.media.get(key)?;
    let ck = (key.to_string(), Arc::as_ptr(bytes) as usize);
    if let Some(v) = image_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&ck) {
        return v.clone();
    }
    let pm = decode_pixmap(bytes).map(Arc::new);
    let mut c = image_cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.len() > 256 {
        c.clear();
    }
    c.insert(ck, pm.clone());
    pm
}

/// Options for a page raster.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub display: DisplayOptions,
    /// Paper colour (page colour from Design › Page Color, else white).
    pub paper: Rgb,
    /// Colour for formatting marks.
    pub mark_color: Rgb,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { display: DisplayOptions::default(), paper: Rgb::WHITE, mark_color: Rgb(0x2B, 0x57, 0x9A) }
    }
}

/// Render a whole page at `scale` pixels per point.
pub fn render_page(doc: &Document, page: &Page, scale: f32, opts: &RenderOptions) -> Rendered {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let w = ((page.w * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let h = ((page.h.min(1e6) * scale).ceil() as u32).clamp(1, MAX_SIDE);
    render_region(doc, page, w, h, Affine::scale(scale as f64), opts)
}

/// Render a page into a `w`×`h` raster with `view` (page points → pixels).
pub fn render_region(doc: &Document, page: &Page, w: u32, h: u32, view: Affine, opts: &RenderOptions) -> Rendered {
    let (w16, h16) = (w.clamp(1, MAX_SIDE) as u16, h.clamp(1, MAX_SIDE) as u16);
    let mut ctx = RenderContext::new_with(w16, h16, vello_cpu::RenderSettings { num_threads: default_threads(), ..Default::default() });
    ctx.set_transform(Affine::IDENTITY);
    ctx.set_paint(color(doc.settings.page_color.unwrap_or(opts.paper), 1.0));
    ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w16 as f64, h16 as f64));
    let items = page_display(doc, page, &opts.display);
    let visible = view.inverse().transform_rect_bbox(kurbo::Rect::new(0.0, 0.0, w16 as f64, h16 as f64)).inflate(40.0, 40.0);
    if let Some(wm) = &doc.settings.watermark {
        draw_watermark(&mut ctx, view, page, wm);
    }
    for it in &items {
        draw(&mut ctx, doc, it, view, &visible, opts);
    }
    ctx.flush();
    let mut pixels = vec![0u8; w16 as usize * h16 as usize * 4];
    let mut res = Resources::new();
    if let Some(pm) = vello_cpu::PixmapMut::new(w16, h16, &mut pixels) {
        ctx.render(pm, &mut res);
    }
    Rendered { width: w16 as u32, height: h16 as u32, pixels }
}

fn draw_watermark(ctx: &mut RenderContext, view: Affine, page: &Page, wm: &wordcraft_doc::Watermark) {
    let r = wordcraft_fonts::word::resolve(&wm.font, false, false);
    let face = r.face;
    let glyphs = wordcraft_fonts::shape(&face, &wm.text, &[], |c| c);
    let upem = face.upem.max(1.0);
    let raw_w: f64 = glyphs.iter().map(|g| g.x_advance as f64).sum::<f64>() / upem;
    if raw_w <= 0.0 {
        return;
    }
    let diag = if wm.diagonal { ((page.w as f64).powi(2) + (page.h.min(2000.0) as f64).powi(2)).sqrt() } else { page.w as f64 };
    let size = (diag * 0.5 / raw_w).min(200.0);
    let k = size / upem;
    let angle = if wm.diagonal { -((page.h.min(2000.0) as f64) / page.w.max(1.0) as f64).atan() } else { 0.0 };
    let base = Affine::translate((page.w as f64 / 2.0, page.h.min(2000.0) as f64 / 2.0))
        * Affine::rotate(angle)
        * Affine::translate((-raw_w * size / 2.0, size * 0.35));
    ctx.set_paint(color(wm.color, if wm.semitransparent { 0.35 } else { 0.8 }));
    let db = FontDb::global();
    let mut x = 0.0;
    for g in &glyphs {
        let o = db.outline(&face, g.gid);
        ctx.set_transform(view * base * Affine::translate((x, 0.0)) * Affine::scale(k));
        ctx.fill_path(&o);
        x += g.x_advance as f64 * k;
    }
}

fn draw(ctx: &mut RenderContext, doc: &Document, it: &Draw, view: Affine, visible: &kurbo::Rect, opts: &RenderOptions) {
    match it {
        Draw::Fill { rect, color: c, alpha } => {
            let r = kurbo::Rect::new(rect.x as f64, rect.y as f64, rect.right() as f64, rect.bottom() as f64);
            if !r.overlaps(*visible) {
                return;
            }
            ctx.set_transform(view);
            ctx.set_paint(color(*c, *alpha));
            ctx.fill_rect(&r);
        }
        Draw::Line { x0, y0, x1, y1, width, color: c, stroke, alpha } => {
            ctx.set_transform(view);
            ctx.set_paint(color(*c, *alpha));
            let w = *width as f64;
            let mut st = kurbo::Stroke::new(w);
            match stroke {
                Stroke::Dotted => st = st.with_dashes(0.0, [w, w * 2.0]),
                Stroke::Dashed => st = st.with_dashes(0.0, [w * 4.0, w * 2.0]),
                _ => {}
            }
            let line = |ctx: &mut RenderContext, dy: f64| {
                let mut p = BezPath::new();
                p.move_to((*x0 as f64, *y0 as f64 + dy));
                p.line_to((*x1 as f64, *y1 as f64 + dy));
                ctx.stroke_path(&p);
            };
            if *stroke == Stroke::Wave {
                let mut p = BezPath::new();
                let (a, b) = ((*x0).min(*x1) as f64, (*x0).max(*x1) as f64);
                let y = *y0 as f64;
                let amp = (w * 1.2).max(0.8);
                let step = amp * 2.0;
                p.move_to((a, y));
                let mut x = a;
                let mut up = true;
                while x < b && x - a < 20_000.0 {
                    let nx = (x + step).min(b);
                    p.quad_to(((x + nx) / 2.0, if up { y - amp } else { y + amp }), (nx, y));
                    x = nx;
                    up = !up;
                }
                ctx.set_stroke(kurbo::Stroke::new(w * 0.8));
                ctx.stroke_path(&p);
                return;
            }
            if *stroke == Stroke::Double {
                ctx.set_stroke(kurbo::Stroke::new(w * 0.6));
                line(ctx, -w * 0.9);
                line(ctx, w * 0.9);
                return;
            }
            ctx.set_stroke(st);
            line(ctx, 0.0);
        }
        Draw::Glyphs { face, size, glyphs, color: c, alpha, synth_bold, synth_italic, .. } => {
            let db = FontDb::global();
            let k = *size as f64 / face.upem.max(1.0);
            ctx.set_paint(color(*c, *alpha));
            let skew = if *synth_italic { Affine::new([1.0, 0.0, -0.21, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
            if *synth_bold {
                ctx.set_stroke(kurbo::Stroke::new(face.upem * 0.03));
            }
            for (gid, x, y) in glyphs {
                let (gx, gy) = (*x as f64, *y as f64);
                if gx < visible.x0 - 100.0 || gx > visible.x1 || gy < visible.y0 || gy > visible.y1 + 200.0 {
                    continue;
                }
                let o = db.outline(face, *gid);
                if o.elements().is_empty() {
                    continue;
                }
                ctx.set_transform(view * Affine::translate((gx, gy)) * skew * Affine::scale(k));
                ctx.fill_path(&o);
                if *synth_bold {
                    ctx.stroke_path(&o);
                }
            }
        }
        Draw::Mark { x, baseline, size, ch } => {
            let face = wordcraft_fonts::word::resolve("Source Sans 3", false, false).face;
            let gid = face.glyph_for(*ch);
            let face = if gid == 0 {
                match FontDb::global().fallback_for(*ch, face.id()) {
                    Some(f) => wordcraft_fonts::FaceRef::of(&f),
                    None => return,
                }
            } else {
                face
            };
            let gid = face.glyph_for(*ch);
            let o = FontDb::global().outline(&face, gid);
            let k = *size as f64 / face.upem.max(1.0);
            ctx.set_paint(color(opts.mark_color, 0.9));
            ctx.set_transform(view * Affine::translate((*x as f64, *baseline as f64)) * Affine::scale(k));
            ctx.fill_path(&o);
        }
        Draw::Image { rect, media, crop, alpha } => {
            let r = kurbo::Rect::new(rect.x as f64, rect.y as f64, rect.right() as f64, rect.bottom() as f64);
            if !r.overlaps(*visible) {
                return;
            }
            let Some(pm) = pixmap_for(doc, media) else {
                ctx.set_transform(view);
                ctx.set_paint(color(Rgb(0xD0, 0xD0, 0xD0), 1.0));
                ctx.fill_rect(&r);
                return;
            };
            let (pw, ph) = (pm.width().max(1) as f64, pm.height().max(1) as f64);
            let [cl, ct, cr, cb] = crop.map(|v| if v.is_finite() { v.clamp(0.0, 0.95) as f64 } else { 0.0 });
            let vis_w = (1.0 - cl - cr).max(0.05);
            let vis_h = (1.0 - ct - cb).max(0.05);
            let sx = r.width() / (pw * vis_w);
            let sy = r.height() / (ph * vis_h);
            ctx.set_transform(view);
            // vello_cpu panics on image paints with sampler alpha != 1, so fade through a layer.
            let alpha = if alpha.is_finite() { alpha.clamp(0.0, 1.0) } else { 1.0 };
            let faded = alpha < 1.0;
            if faded {
                ctx.push_opacity_layer(alpha);
            }
            ctx.set_paint(vello_cpu::Image {
                image: vello_cpu::ImageSource::Pixmap(pm),
                sampler: peniko::ImageSampler::default().with_quality(peniko::ImageQuality::Medium),
            });
            ctx.set_paint_transform(Affine::translate((r.x0 - cl * pw * sx, r.y0 - ct * ph * sy)) * Affine::scale_non_uniform(sx, sy));
            ctx.fill_rect(&r);
            ctx.reset_paint_transform();
            if faded {
                ctx.pop_layer();
            }
        }
        Draw::Shape { rect, kind, fill, stroke, stroke_width } => {
            let r = kurbo::Rect::new(rect.x as f64, rect.y as f64, rect.right() as f64, rect.bottom() as f64);
            let path = shape_path(*kind, r);
            ctx.set_transform(view);
            if !kind.is_line_like()
                && let Some(f) = fill
            {
                ctx.set_paint(color(*f, 1.0));
                ctx.fill_path(&path);
            }
            if let Some(s) = stroke {
                ctx.set_paint(color(*s, 1.0));
                ctx.set_stroke(kurbo::Stroke::new(stroke_width.max(0.25) as f64));
                ctx.stroke_path(&path);
            }
        }
    }
}

/// Outline of a basic shape in a rectangle.
pub fn shape_path(kind: ShapeKind, r: kurbo::Rect) -> BezPath {
    let (cx, cy) = (r.center().x, r.center().y);
    let (w, h) = (r.width().max(1.0), r.height().max(1.0));
    let (x0, y0, x1, y1) = (r.x0, r.y0, r.x1, r.y1);

    let poly = |pts: &[(f64, f64)]| {
        let mut p = BezPath::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                p.move_to((*x, *y));
            } else {
                p.line_to((*x, *y));
            }
        }
        p.close_path();
        p
    };

    let ngon = |sides: usize, rot: f64| {
        let mut pts = Vec::with_capacity(sides);
        let rx = w / 2.0;
        let ry = h / 2.0;
        for i in 0..sides {
            let a = rot + 2.0 * std::f64::consts::PI * (i as f64) / (sides as f64);
            pts.push((cx + rx * a.cos(), cy + ry * a.sin()));
        }
        poly(&pts)
    };

    let star = |pts_count: usize, inner_ratio: f64| {
        let mut pts = Vec::with_capacity(pts_count * 2);
        let rx = w / 2.0;
        let ry = h / 2.0;
        for i in 0..(pts_count * 2) {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / (pts_count as f64);
            let rad = if i % 2 == 0 { 1.0 } else { inner_ratio };
            pts.push((cx + rx * rad * a.cos(), cy + ry * rad * a.sin()));
        }
        poly(&pts)
    };

    match kind {
        ShapeKind::Rectangle | ShapeKind::TextBox | ShapeKind::DrawingCanvas | ShapeKind::FlowchartProcess => r.to_path(0.1),
        ShapeKind::RoundedRectangle | ShapeKind::FlowchartAlternateProcess => kurbo::RoundedRect::from_rect(r, w.min(h) * 0.16).to_path(0.1),
        ShapeKind::SingleCornerSnipped => {
            let cut = w.min(h) * 0.2;
            poly(&[(x0 + cut, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0 + cut)])
        }
        ShapeKind::TopCornersSnipped => {
            let cut = w.min(h) * 0.2;
            poly(&[(x0 + cut, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1), (x0, y1), (x0, y0 + cut)])
        }
        ShapeKind::DiagonalCornersSnipped => {
            let cut = w.min(h) * 0.2;
            poly(&[(x0 + cut, y0), (x1, y0), (x1, y1 - cut), (x1 - cut, y1), (x0, y1), (x0, y0 + cut)])
        }
        ShapeKind::SnippedAndRounded => {
            let cut = w.min(h) * 0.2;
            poly(&[(x0 + cut, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1), (x0, y1)])
        }
        ShapeKind::RoundSingleCorner => {
            let rad = w.min(h) * 0.25;
            let mut p = BezPath::new();
            p.move_to((x0 + rad, y0));
            p.line_to((x1, y0));
            p.line_to((x1, y1));
            p.line_to((x0, y1));
            p.line_to((x0, y0 + rad));
            p.quad_to((x0, y0), (x0 + rad, y0));
            p.close_path();
            p
        }
        ShapeKind::RoundSameSideCorner => {
            let rad = w.min(h) * 0.25;
            let mut p = BezPath::new();
            p.move_to((x0 + rad, y0));
            p.line_to((x1 - rad, y0));
            p.quad_to((x1, y0), (x1, y0 + rad));
            p.line_to((x1, y1));
            p.line_to((x0, y1));
            p.line_to((x0, y0 + rad));
            p.quad_to((x0, y0), (x0 + rad, y0));
            p.close_path();
            p
        }
        ShapeKind::RoundDiagonalCorner => {
            let rad = w.min(h) * 0.25;
            let mut p = BezPath::new();
            p.move_to((x0 + rad, y0));
            p.line_to((x1, y0));
            p.line_to((x1, y1 - rad));
            p.quad_to((x1, y1), (x1 - rad, y1));
            p.line_to((x0, y1));
            p.line_to((x0, y0 + rad));
            p.quad_to((x0, y0), (x0 + rad, y0));
            p.close_path();
            p
        }
        ShapeKind::Ellipse | ShapeKind::FlowchartConnector => kurbo::Ellipse::from_rect(r).to_path(0.1),
        ShapeKind::Triangle | ShapeKind::FlowchartExtract => poly(&[(cx, y0), (x1, y1), (x0, y1)]),
        ShapeKind::RightTriangle => poly(&[(x0, y0), (x1, y1), (x0, y1)]),
        ShapeKind::Parallelogram | ShapeKind::FlowchartData => {
            let off = w * 0.2;
            poly(&[(x0 + off, y0), (x1, y0), (x1 - off, y1), (x0, y1)])
        }
        ShapeKind::Trapezoid | ShapeKind::FlowchartManualOperation => {
            let off = w * 0.2;
            poly(&[(x0 + off, y0), (x1 - off, y0), (x1, y1), (x0, y1)])
        }
        ShapeKind::Diamond | ShapeKind::FlowchartDecision => poly(&[(cx, y0), (x1, cy), (cx, y1), (x0, cy)]),
        ShapeKind::Pentagon => ngon(5, -std::f64::consts::FRAC_PI_2),
        ShapeKind::Hexagon | ShapeKind::FlowchartPreparation => ngon(6, 0.0),
        ShapeKind::Heptagon => ngon(7, -std::f64::consts::FRAC_PI_2),
        ShapeKind::Octagon => ngon(8, std::f64::consts::PI / 8.0),
        ShapeKind::Decagon => ngon(10, -std::f64::consts::FRAC_PI_2),
        ShapeKind::Dodecagon => ngon(12, 0.0),

        ShapeKind::Pie => {
            let mut p = BezPath::new();
            p.move_to((cx, cy));
            p.line_to((x1, cy));
            let mut sub = BezPath::new();
            let ell = kurbo::Ellipse::from_rect(r);
            sub.extend(ell.to_path(0.1).elements().iter().copied());
            p.extend(sub.elements().iter().copied());
            p.close_path();
            p
        }
        ShapeKind::Chord => {
            let mut p = BezPath::new();
            p.move_to((x0, cy));
            p.quad_to((cx, y1), (x1, cy));
            p.close_path();
            p
        }
        ShapeKind::Teardrop => {
            let mut p = BezPath::new();
            p.move_to((cx, y0));
            p.quad_to((x1, y0), (x1, cy));
            p.quad_to((x1, y1), (cx, y1));
            p.quad_to((x0, y1), (x0, cy));
            p.line_to((cx, y0));
            p.close_path();
            p
        }
        ShapeKind::Frame => {
            let th = w.min(h) * 0.15;
            let mut p = r.to_path(0.1);
            p.extend(kurbo::Rect::new(x0 + th, y0 + th, x1 - th, y1 - th).to_path(0.1).elements().iter().copied());
            p
        }
        ShapeKind::HalfFrame => {
            let th = w.min(h) * 0.15;
            poly(&[(x0, y0), (x1, y0), (x1, y0 + th), (x0 + th, y0 + th), (x0 + th, y1), (x0, y1)])
        }
        ShapeKind::LShape => {
            let th = w.min(h) * 0.25;
            poly(&[(x0, y0), (x0 + th, y0), (x0 + th, y1 - th), (x1, y1 - th), (x1, y1), (x0, y1)])
        }
        ShapeKind::DiagonalStripe => {
            let th = w.min(h) * 0.3;
            poly(&[(x0 + th, y0), (x1, y1 - th), (x1, y1), (x1 - th, y1), (x0, y0 + th), (x0, y0)])
        }
        ShapeKind::Cross | ShapeKind::MathPlus => {
            let xw = w * 0.3;
            let yh = h * 0.3;
            poly(&[
                (cx - xw / 2.0, y0),
                (cx + xw / 2.0, y0),
                (cx + xw / 2.0, cy - yh / 2.0),
                (x1, cy - yh / 2.0),
                (x1, cy + yh / 2.0),
                (cx + xw / 2.0, cy + yh / 2.0),
                (cx + xw / 2.0, y1),
                (cx - xw / 2.0, y1),
                (cx - xw / 2.0, cy + yh / 2.0),
                (x0, cy + yh / 2.0),
                (x0, cy - yh / 2.0),
                (cx - xw / 2.0, cy - yh / 2.0),
            ])
        }
        ShapeKind::Plaque => {
            let cut = w.min(h) * 0.2;
            poly(&[(x0 + cut, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1 - cut), (x1 - cut, y1), (x0 + cut, y1), (x0, y1 - cut), (x0, y0 + cut)])
        }
        ShapeKind::Cylinder | ShapeKind::FlowchartMagneticDisk => {
            let cap = h * 0.2;
            let mut p = BezPath::new();
            p.move_to((x0, y0 + cap / 2.0));
            p.quad_to((cx, y0 - cap / 2.0), (x1, y0 + cap / 2.0));
            p.line_to((x1, y1 - cap / 2.0));
            p.quad_to((cx, y1 + cap / 2.0), (x0, y1 - cap / 2.0));
            p.close_path();
            p
        }
        ShapeKind::Cube => {
            let off = w.min(h) * 0.25;
            let mut p = poly(&[(x0, y0 + off), (x1 - off, y0 + off), (x1 - off, y1), (x0, y1)]);
            p.move_to((x0, y0 + off));
            p.line_to((x0 + off, y0));
            p.line_to((x1, y0));
            p.line_to((x1 - off, y0 + off));
            p.move_to((x1, y0));
            p.line_to((x1, y1 - off));
            p.line_to((x1 - off, y1));
            p
        }
        ShapeKind::Bevel => {
            let th = w.min(h) * 0.15;
            let mut p = r.to_path(0.1);
            p.move_to((x0, y0));
            p.line_to((x0 + th, y0 + th));
            p.move_to((x1, y0));
            p.line_to((x1 - th, y0 + th));
            p.move_to((x1, y1));
            p.line_to((x1 - th, y1 - th));
            p.move_to((x0, y1));
            p.line_to((x0 + th, y1 - th));
            p
        }
        ShapeKind::Donut => {
            let mut p = kurbo::Ellipse::from_rect(r).to_path(0.1);
            p.extend(
                kurbo::Ellipse::from_rect(kurbo::Rect::new(x0 + w * 0.2, y0 + h * 0.2, x1 - w * 0.2, y1 - h * 0.2))
                    .to_path(0.1)
                    .elements()
                    .iter()
                    .copied(),
            );
            p
        }
        ShapeKind::NoSymbol => {
            let mut p = kurbo::Ellipse::from_rect(r).to_path(0.1);
            p.move_to((x0 + w * 0.15, y0 + h * 0.15));
            p.line_to((x1 - w * 0.15, y1 - h * 0.15));
            p
        }
        ShapeKind::BlockArc => {
            let mut p = BezPath::new();
            p.move_to((x0, cy));
            p.quad_to((x0, y0), (cx, y0));
            p.line_to((cx, y0 + h * 0.2));
            p.quad_to((x0 + w * 0.2, y0 + h * 0.2), (x0 + w * 0.2, cy));
            p.close_path();
            p
        }
        ShapeKind::FoldedCorner => {
            let cut = w.min(h) * 0.25;
            poly(&[(x0, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1), (x0, y1)])
        }
        ShapeKind::SmileyFace => {
            let mut p = kurbo::Ellipse::from_rect(r).to_path(0.1);
            p.move_to((cx - w * 0.2, cy - h * 0.15));
            p.line_to((cx - w * 0.2, cy - h * 0.05));
            p.move_to((cx + w * 0.2, cy - h * 0.15));
            p.line_to((cx + w * 0.2, cy - h * 0.05));
            p.move_to((cx - w * 0.25, cy + h * 0.15));
            p.quad_to((cx, cy + h * 0.35), (cx + w * 0.25, cy + h * 0.15));
            p
        }
        ShapeKind::Heart => {
            let mut p = BezPath::new();
            p.move_to((cx, y0 + h * 0.3));
            p.curve_to((cx, y0), (x0, y0), (x0, y0 + h * 0.3));
            p.curve_to((x0, y0 + h * 0.6), (cx - w * 0.1, y0 + h * 0.75), (cx, y1));
            p.curve_to((cx + w * 0.1, y0 + h * 0.75), (x1, y0 + h * 0.6), (x1, y0 + h * 0.3));
            p.curve_to((x1, y0), (cx, y0), (cx, y0 + h * 0.3));
            p.close_path();
            p
        }
        ShapeKind::LightningBolt => poly(&[
            (cx + w * 0.1, y0),
            (x0 + w * 0.1, cy + h * 0.05),
            (cx, cy + h * 0.05),
            (cx - w * 0.1, y1),
            (x1 - w * 0.1, cy - h * 0.05),
            (cx, cy - h * 0.05),
        ]),
        ShapeKind::Sun => {
            let mut p = star(12, 0.75);
            p.extend(
                kurbo::Ellipse::from_rect(kurbo::Rect::new(x0 + w * 0.2, y0 + h * 0.2, x1 - w * 0.2, y1 - h * 0.2))
                    .to_path(0.1)
                    .elements()
                    .iter()
                    .copied(),
            );
            p
        }
        ShapeKind::Moon => {
            let mut p = BezPath::new();
            p.move_to((cx, y0));
            p.quad_to((x1, cy), (cx, y1));
            p.quad_to((cx + w * 0.15, cy), (cx, y0));
            p.close_path();
            p
        }
        ShapeKind::Cloud | ShapeKind::CloudCallout => {
            let mut p = BezPath::new();
            p.move_to((x0 + w * 0.2, y1 - h * 0.2));
            p.quad_to((x0, y1 - h * 0.2), (x0, cy));
            p.quad_to((x0, y0 + h * 0.1), (x0 + w * 0.3, y0 + h * 0.15));
            p.quad_to((cx, y0), (cx + w * 0.2, y0 + h * 0.15));
            p.quad_to((x1, y0 + h * 0.1), (x1, cy));
            p.quad_to((x1, y1 - h * 0.2), (x1 - w * 0.2, y1 - h * 0.2));
            p.close_path();
            p
        }

        // Lines & Connectors
        ShapeKind::Line => {
            let mut p = BezPath::new();
            p.move_to((x0, y1));
            p.line_to((x1, y0));
            p
        }
        ShapeKind::Arrow => poly(&[
            (x0, cy - h * 0.2),
            (x1 - h * 0.5, cy - h * 0.2),
            (x1 - h * 0.5, y0),
            (x1, cy),
            (x1 - h * 0.5, y1),
            (x1 - h * 0.5, cy + h * 0.2),
            (x0, cy + h * 0.2),
        ]),
        ShapeKind::DoubleArrow => {
            let ah = h * 0.4;
            poly(&[
                (x0 + ah, y0),
                (x0, cy),
                (x0 + ah, y1),
                (x0 + ah, cy + h * 0.15),
                (x1 - ah, cy + h * 0.15),
                (x1 - ah, y1),
                (x1, cy),
                (x1 - ah, y0),
                (x1 - ah, cy - h * 0.15),
                (x0 + ah, cy - h * 0.15),
            ])
        }
        ShapeKind::ElbowConnector | ShapeKind::ElbowArrow | ShapeKind::ElbowDoubleArrow => {
            let mut p = BezPath::new();
            p.move_to((x0, y0));
            p.line_to((cx, y0));
            p.line_to((cx, y1));
            p.line_to((x1, y1));
            if kind == ShapeKind::ElbowArrow || kind == ShapeKind::ElbowDoubleArrow {
                p.move_to((x1 - 6.0, y1 - 4.0));
                p.line_to((x1, y1));
                p.line_to((x1 - 6.0, y1 + 4.0));
            }
            if kind == ShapeKind::ElbowDoubleArrow {
                p.move_to((x0 + 6.0, y0 - 4.0));
                p.line_to((x0, y0));
                p.line_to((x0 + 6.0, y0 + 4.0));
            }
            p
        }
        ShapeKind::CurvedConnector | ShapeKind::CurvedArrow | ShapeKind::CurvedDoubleArrow | ShapeKind::Curve | ShapeKind::Arc => {
            let mut p = BezPath::new();
            p.move_to((x0, y1));
            p.quad_to((x0, y0), (x1, y0));
            if kind == ShapeKind::CurvedArrow || kind == ShapeKind::CurvedDoubleArrow {
                p.move_to((x1 - 6.0, y0 - 4.0));
                p.line_to((x1, y0));
                p.line_to((x1 - 6.0, y0 + 4.0));
            }
            p
        }
        ShapeKind::Freeform | ShapeKind::Scribble => {
            let mut p = BezPath::new();
            p.move_to((x0, cy));
            p.quad_to((x0 + w * 0.25, y0), (cx, cy));
            p.quad_to((x0 + w * 0.75, y1), (x1, cy));
            p
        }
        ShapeKind::DoubleBracket | ShapeKind::LeftBracket | ShapeKind::RightBracket => {
            let mut p = BezPath::new();
            p.move_to((x0 + w * 0.3, y0));
            p.line_to((x0, y0));
            p.line_to((x0, y1));
            p.line_to((x0 + w * 0.3, y1));
            if kind != ShapeKind::LeftBracket {
                p.move_to((x1 - w * 0.3, y0));
                p.line_to((x1, y0));
                p.line_to((x1, y1));
                p.line_to((x1 - w * 0.3, y1));
            }
            p
        }
        ShapeKind::DoubleBrace | ShapeKind::LeftBrace | ShapeKind::RightBrace => {
            let mut p = BezPath::new();
            p.move_to((x0 + w * 0.3, y0));
            p.quad_to((x0 + w * 0.15, y0), (x0 + w * 0.15, cy - h * 0.1));
            p.line_to((x0, cy));
            p.line_to((x0 + w * 0.15, cy + h * 0.1));
            p.quad_to((x0 + w * 0.15, y1), (x0 + w * 0.3, y1));
            if kind != ShapeKind::LeftBrace {
                p.move_to((x1 - w * 0.3, y0));
                p.quad_to((x1 - w * 0.15, y0), (x1 - w * 0.15, cy - h * 0.1));
                p.line_to((x1, cy));
                p.line_to((x1 - w * 0.15, cy + h * 0.1));
                p.quad_to((x1 - w * 0.15, y1), (x1 - w * 0.3, y1));
            }
            p
        }

        // Block Arrows
        ShapeKind::LeftArrow => poly(&[
            (x0, cy),
            (x0 + h * 0.5, y0),
            (x0 + h * 0.5, cy - h * 0.2),
            (x1, cy - h * 0.2),
            (x1, cy + h * 0.2),
            (x0 + h * 0.5, cy + h * 0.2),
            (x0 + h * 0.5, y1),
        ]),
        ShapeKind::UpArrow => poly(&[
            (cx, y0),
            (x1, y0 + w * 0.5),
            (cx + w * 0.2, y0 + w * 0.5),
            (cx + w * 0.2, y1),
            (cx - w * 0.2, y1),
            (cx - w * 0.2, y0 + w * 0.5),
            (x0, y0 + w * 0.5),
        ]),
        ShapeKind::DownArrow => poly(&[
            (cx - w * 0.2, y0),
            (cx + w * 0.2, y0),
            (cx + w * 0.2, y1 - w * 0.5),
            (x1, y1 - w * 0.5),
            (cx, y1),
            (x0, y1 - w * 0.5),
            (cx - w * 0.2, y1 - w * 0.5),
        ]),
        ShapeKind::LeftRightArrow => {
            let ah = h * 0.5;
            poly(&[
                (x0, cy),
                (x0 + ah, y0),
                (x0 + ah, cy - h * 0.2),
                (x1 - ah, cy - h * 0.2),
                (x1 - ah, y0),
                (x1, cy),
                (x1 - ah, y1),
                (x1 - ah, cy + h * 0.2),
                (x0 + ah, cy + h * 0.2),
                (x0 + ah, y1),
            ])
        }
        ShapeKind::UpDownArrow => {
            let aw = w * 0.5;
            poly(&[
                (cx, y0),
                (x1, y0 + aw),
                (cx + w * 0.2, y0 + aw),
                (cx + w * 0.2, y1 - aw),
                (x1, y1 - aw),
                (cx, y1),
                (x0, y1 - aw),
                (cx - w * 0.2, y1 - aw),
                (cx - w * 0.2, y0 + aw),
                (x0, y0 + aw),
            ])
        }
        ShapeKind::QuadArrow => {
            let mut p = BezPath::new();
            p.move_to((cx, y0));
            p.line_to((cx + w * 0.15, y0 + h * 0.15));
            p.line_to((cx + w * 0.05, y0 + h * 0.15));
            p.line_to((cx + w * 0.05, cy - h * 0.05));
            p.line_to((x1 - w * 0.15, cy - h * 0.05));
            p.line_to((x1 - w * 0.15, cy - h * 0.15));
            p.line_to((x1, cy));
            p.line_to((x1 - w * 0.15, cy + h * 0.15));
            p.line_to((x1 - w * 0.15, cy + h * 0.05));
            p.line_to((cx + w * 0.05, cy + h * 0.05));
            p.line_to((cx + w * 0.05, y1 - h * 0.15));
            p.line_to((cx + w * 0.15, y1 - h * 0.15));
            p.line_to((cx, y1));
            p.line_to((cx - w * 0.15, y1 - h * 0.15));
            p.line_to((cx - w * 0.05, y1 - h * 0.15));
            p.line_to((cx - w * 0.05, cy + h * 0.05));
            p.line_to((x0 + w * 0.15, cy + h * 0.05));
            p.line_to((x0 + w * 0.15, cy + h * 0.15));
            p.line_to((x0, cy));
            p.line_to((x0 + w * 0.15, cy - h * 0.15));
            p.line_to((x0 + w * 0.15, cy - h * 0.05));
            p.line_to((cx - w * 0.05, cy - h * 0.05));
            p.line_to((cx - w * 0.05, y0 + h * 0.15));
            p.line_to((cx - w * 0.15, y0 + h * 0.15));
            p.close_path();
            p
        }
        ShapeKind::LeftRightUpArrow | ShapeKind::BentArrow | ShapeKind::UTurnArrow | ShapeKind::LeftUpArrow | ShapeKind::BentUpArrow => poly(&[
            (x0, cy),
            (x0 + w * 0.3, y0),
            (x0 + w * 0.3, cy - h * 0.2),
            (x1 - w * 0.2, cy - h * 0.2),
            (x1 - w * 0.2, y1),
            (x1, y1),
            (x1, cy - h * 0.4),
            (x0 + w * 0.3, cy - h * 0.4),
            (x0 + w * 0.3, y0),
        ]),
        ShapeKind::StripedRightArrow => {
            let mut p = poly(&[
                (x0 + w * 0.2, cy - h * 0.2),
                (x1 - h * 0.5, cy - h * 0.2),
                (x1 - h * 0.5, y0),
                (x1, cy),
                (x1 - h * 0.5, y1),
                (x1 - h * 0.5, cy + h * 0.2),
                (x0 + w * 0.2, cy + h * 0.2),
            ]);
            p.move_to((x0, cy - h * 0.2));
            p.line_to((x0 + w * 0.1, cy - h * 0.2));
            p.line_to((x0 + w * 0.1, cy + h * 0.2));
            p.line_to((x0, cy + h * 0.2));
            p.close_path();
            p
        }
        ShapeKind::NotchedRightArrow => poly(&[
            (x0, y0 + h * 0.2),
            (x0 + w * 0.15, cy),
            (x0, y1 - h * 0.2),
            (x1 - h * 0.5, y1 - h * 0.2),
            (x1 - h * 0.5, y1),
            (x1, cy),
            (x1 - h * 0.5, y0),
            (x1 - h * 0.5, y0 + h * 0.2),
        ]),
        ShapeKind::PentagonArrow => {
            poly(&[(x0, y0 + h * 0.15), (x1 - w * 0.3, y0 + h * 0.15), (x1, cy), (x1 - w * 0.3, y1 - h * 0.15), (x0, y1 - h * 0.15)])
        }
        ShapeKind::Chevron => {
            let cut = w * 0.3;
            poly(&[(x0, y0), (x1 - cut, y0), (x1, cy), (x1 - cut, y1), (x0, y1), (x0 + cut, cy)])
        }
        ShapeKind::CircularArrow => {
            let mut p = BezPath::new();
            p.move_to((cx, y0));
            p.quad_to((x1, y0), (x1, cy));
            p.quad_to((x1, y1), (cx, y1));
            p.line_to((cx, y1 - h * 0.2));
            p.quad_to((x1 - w * 0.2, y1 - h * 0.2), (x1 - w * 0.2, cy));
            p.quad_to((x1 - w * 0.2, y0 + h * 0.2), (cx, y0 + h * 0.2));
            p.line_to((cx, y0 + h * 0.35));
            p.line_to((cx - w * 0.2, y0 + h * 0.1));
            p.line_to((cx, y0));
            p.close_path();
            p
        }
        ShapeKind::RightArrowCallout
        | ShapeKind::DownArrowCallout
        | ShapeKind::LeftArrowCallout
        | ShapeKind::UpArrowCallout
        | ShapeKind::LeftRightArrowCallout
        | ShapeKind::QuadArrowCallout => {
            poly(&[(x0, y0), (x1 - w * 0.3, y0), (x1 - w * 0.3, y0 + h * 0.2), (x1, cy), (x1 - w * 0.3, y1 - h * 0.2), (x1 - w * 0.3, y1), (x0, y1)])
        }

        // Equation Shapes
        ShapeKind::MathMinus => {
            let bar_h = h * 0.25;
            kurbo::Rect::new(x0, cy - bar_h / 2.0, x1, cy + bar_h / 2.0).to_path(0.1)
        }
        ShapeKind::MathMultiply => {
            let mut p = BezPath::new();
            let th = w.min(h) * 0.2;
            p.move_to((x0, y0 + th));
            p.line_to((x0 + th, y0));
            p.line_to((x1, y1 - th));
            p.line_to((x1 - th, y1));
            p.close_path();
            p.move_to((x1 - th, y0));
            p.line_to((x1, y0 + th));
            p.line_to((x0 + th, y1));
            p.line_to((x0, y1 - th));
            p.close_path();
            p
        }
        ShapeKind::MathDivide => {
            let bar_h = h * 0.18;
            let mut p = kurbo::Rect::new(x0, cy - bar_h / 2.0, x1, cy + bar_h / 2.0).to_path(0.1);
            let dot_r = w.min(h) * 0.1;
            p.extend(
                kurbo::Ellipse::from_rect(kurbo::Rect::from_center_size(kurbo::Point::new(cx, y0 + h * 0.2), (dot_r * 2.0, dot_r * 2.0)))
                    .to_path(0.1)
                    .elements()
                    .iter()
                    .copied(),
            );
            p.extend(
                kurbo::Ellipse::from_rect(kurbo::Rect::from_center_size(kurbo::Point::new(cx, y1 - h * 0.2), (dot_r * 2.0, dot_r * 2.0)))
                    .to_path(0.1)
                    .elements()
                    .iter()
                    .copied(),
            );
            p
        }
        ShapeKind::MathEqual => {
            let mut p = kurbo::Rect::new(x0, cy - h * 0.25, x1, cy - h * 0.05).to_path(0.1);
            p.extend(kurbo::Rect::new(x0, cy + h * 0.05, x1, cy + h * 0.25).to_path(0.1).elements().iter().copied());
            p
        }
        ShapeKind::MathNotEqual => {
            let mut p = shape_path(ShapeKind::MathEqual, r);
            p.move_to((x0 + w * 0.7, y0));
            p.line_to((x0 + w * 0.3, y1));
            p
        }

        // Flowchart
        ShapeKind::FlowchartPredefinedProcess => {
            let mut p = r.to_path(0.1);
            let off = w * 0.15;
            p.move_to((x0 + off, y0));
            p.line_to((x0 + off, y1));
            p.move_to((x1 - off, y0));
            p.line_to((x1 - off, y1));
            p
        }
        ShapeKind::FlowchartInternalStorage => {
            let mut p = r.to_path(0.1);
            let off = w.min(h) * 0.2;
            p.move_to((x0 + off, y0));
            p.line_to((x0 + off, y1));
            p.move_to((x0, y0 + off));
            p.line_to((x1, y0 + off));
            p
        }
        ShapeKind::FlowchartDocument => {
            let mut p = BezPath::new();
            p.move_to((x0, y0));
            p.line_to((x1, y0));
            p.line_to((x1, y1 - h * 0.15));
            p.quad_to((x0 + w * 0.75, y1), (cx, y1 - h * 0.15));
            p.quad_to((x0 + w * 0.25, y1 - h * 0.3), (x0, y1 - h * 0.15));
            p.close_path();
            p
        }
        ShapeKind::FlowchartMultidocument => {
            let mut p = shape_path(ShapeKind::FlowchartDocument, kurbo::Rect::new(x0 + 4.0, y0 + 4.0, x1, y1));
            p.extend(shape_path(ShapeKind::FlowchartDocument, kurbo::Rect::new(x0, y0, x1 - 4.0, y1 - 4.0)).elements().iter().copied());
            p
        }
        ShapeKind::FlowchartTerminator | ShapeKind::FlowchartDelay => {
            let rad = h * 0.5;
            kurbo::RoundedRect::from_rect(r, rad).to_path(0.1)
        }
        ShapeKind::FlowchartManualInput => poly(&[(x0, y0 + h * 0.2), (x1, y0), (x1, y1), (x0, y1)]),
        ShapeKind::FlowchartOffpageConnector => poly(&[(x0, y0), (x1, y0), (x1, y0 + h * 0.65), (cx, y1), (x0, y0 + h * 0.65)]),
        ShapeKind::FlowchartCard => {
            let cut = w.min(h) * 0.25;
            poly(&[(x0 + cut, y0), (x1, y0), (x1, y1), (x0, y1)])
        }
        ShapeKind::FlowchartPunchedTape => {
            let mut p = BezPath::new();
            p.move_to((x0, y0 + h * 0.15));
            p.quad_to((x0 + w * 0.25, y0 + h * 0.3), (cx, y0 + h * 0.15));
            p.quad_to((x0 + w * 0.75, y0), (x1, y0 + h * 0.15));
            p.line_to((x1, y1 - h * 0.15));
            p.quad_to((x0 + w * 0.75, y1), (cx, y1 - h * 0.15));
            p.quad_to((x0 + w * 0.25, y1 - h * 0.3), (x0, y1 - h * 0.15));
            p.close_path();
            p
        }
        ShapeKind::FlowchartSummingJunction => {
            let mut p = kurbo::Ellipse::from_rect(r).to_path(0.1);
            p.move_to((x0 + w * 0.15, y0 + h * 0.15));
            p.line_to((x1 - w * 0.15, y1 - h * 0.15));
            p.move_to((x1 - w * 0.15, y0 + h * 0.15));
            p.line_to((x0 + w * 0.15, y1 - h * 0.15));
            p
        }
        ShapeKind::FlowchartOr => {
            let mut p = kurbo::Ellipse::from_rect(r).to_path(0.1);
            p.move_to((cx, y0));
            p.line_to((cx, y1));
            p.move_to((x0, cy));
            p.line_to((x1, cy));
            p
        }
        ShapeKind::FlowchartCollate | ShapeKind::FlowchartMerge => poly(&[(x0, y0), (x1, y0), (cx, y1)]),
        ShapeKind::FlowchartSort => poly(&[(cx, y0), (x1, cy), (cx, y1), (x0, cy)]),
        ShapeKind::FlowchartStoredData => {
            let mut p = BezPath::new();
            p.move_to((x0 + w * 0.2, y0));
            p.line_to((x1, y0));
            p.quad_to((x1 - w * 0.2, cy), (x1, y1));
            p.line_to((x0 + w * 0.2, y1));
            p.quad_to((x0, cy), (x0 + w * 0.2, y0));
            p.close_path();
            p
        }
        ShapeKind::FlowchartSequentialData => {
            let mut p = kurbo::Ellipse::from_rect(kurbo::Rect::new(x0, y0, x1 - w * 0.2, y1)).to_path(0.1);
            p.move_to((cx, y1));
            p.line_to((x1, y1));
            p
        }
        ShapeKind::FlowchartDirectAccessStorage => {
            let mut p = BezPath::new();
            p.move_to((x0, y0));
            p.line_to((x1 - w * 0.2, y0));
            p.quad_to((x1, cy), (x1 - w * 0.2, y1));
            p.line_to((x0, y1));
            p.quad_to((x0 + w * 0.2, cy), (x0, y0));
            p.close_path();
            p
        }
        ShapeKind::FlowchartDisplay => {
            let mut p = BezPath::new();
            p.move_to((x0 + w * 0.2, y0));
            p.line_to((x1 - w * 0.2, y0));
            p.quad_to((x1, cy), (x1 - w * 0.2, y1));
            p.line_to((x0 + w * 0.2, y1));
            p.line_to((x0, cy));
            p.close_path();
            p
        }

        // Stars & Banners
        ShapeKind::Explosion1 => star(8, 0.5),
        ShapeKind::Explosion2 => star(14, 0.6),
        ShapeKind::Star4 => star(4, 0.35),
        ShapeKind::Star => star(5, 0.4),
        ShapeKind::Star6 => star(6, 0.5),
        ShapeKind::Star7 => star(7, 0.55),
        ShapeKind::Star8 => star(8, 0.55),
        ShapeKind::Star10 => star(10, 0.6),
        ShapeKind::Star12 => star(12, 0.65),
        ShapeKind::Star16 => star(16, 0.7),
        ShapeKind::Star24 => star(24, 0.75),
        ShapeKind::Star32 => star(32, 0.8),
        ShapeKind::UpRibbon => poly(&[
            (x0, y0 + h * 0.3),
            (x0 + w * 0.2, y0),
            (x1 - w * 0.2, y0),
            (x1, y0 + h * 0.3),
            (x1 - w * 0.1, cy),
            (x1, y1),
            (x1 - w * 0.2, y1 - h * 0.3),
            (x0 + w * 0.2, y1 - h * 0.3),
            (x0, y1),
            (x0 + w * 0.1, cy),
        ]),
        ShapeKind::DownRibbon => poly(&[
            (x0, y0),
            (x0 + w * 0.2, y0 + h * 0.3),
            (x1 - w * 0.2, y0 + h * 0.3),
            (x1, y0),
            (x1 - w * 0.1, cy),
            (x1, y1 - h * 0.3),
            (x1 - w * 0.2, y1),
            (x0 + w * 0.2, y1),
            (x0, y1 - h * 0.3),
            (x0 + w * 0.1, cy),
        ]),
        ShapeKind::CurvedUpRibbon | ShapeKind::CurvedDownRibbon => {
            let mut p = BezPath::new();
            p.move_to((x0, y1 - h * 0.2));
            p.quad_to((cx, y0), (x1, y1 - h * 0.2));
            p.line_to((x1, y1));
            p.quad_to((cx, y0 + h * 0.2), (x0, y1));
            p.close_path();
            p
        }
        ShapeKind::VerticalScroll => {
            let mut p = r.to_path(0.1);
            p.move_to((x0, y0 + h * 0.15));
            p.line_to((x1, y0 + h * 0.15));
            p.move_to((x0, y1 - h * 0.15));
            p.line_to((x1, y1 - h * 0.15));
            p
        }
        ShapeKind::HorizontalScroll => {
            let mut p = r.to_path(0.1);
            p.move_to((x0 + w * 0.15, y0));
            p.line_to((x0 + w * 0.15, y1));
            p.move_to((x1 - w * 0.15, y0));
            p.line_to((x1 - w * 0.15, y1));
            p
        }
        ShapeKind::Wave => {
            let mut p = BezPath::new();
            p.move_to((x0, y0 + h * 0.2));
            p.quad_to((x0 + w * 0.25, y0 + h * 0.4), (cx, y0 + h * 0.2));
            p.quad_to((x0 + w * 0.75, y0), (x1, y0 + h * 0.2));
            p.line_to((x1, y1));
            p.quad_to((x0 + w * 0.75, y1 - h * 0.2), (cx, y1));
            p.quad_to((x0 + w * 0.25, y1 + h * 0.2), (x0, y1));
            p.close_path();
            p
        }
        ShapeKind::DoubleWave => {
            let mut p = shape_path(ShapeKind::Wave, r);
            p.move_to((x0, cy));
            p.line_to((x1, cy));
            p
        }

        // Callouts
        ShapeKind::RectangularCallout => poly(&[
            (x0, y0),
            (x1, y0),
            (x1, y1 - h * 0.25),
            (x0 + w * 0.4, y1 - h * 0.25),
            (x0 + w * 0.2, y1),
            (x0 + w * 0.25, y1 - h * 0.25),
            (x0, y1 - h * 0.25),
        ]),
        ShapeKind::RoundedRectangularCallout => {
            let mut p = kurbo::RoundedRect::from_rect(kurbo::Rect::new(x0, y0, x1, y1 - h * 0.25), 6.0).to_path(0.1);
            p.move_to((x0 + w * 0.4, y1 - h * 0.25));
            p.line_to((x0 + w * 0.2, y1));
            p.line_to((x0 + w * 0.25, y1 - h * 0.25));
            p
        }
        ShapeKind::OvalCallout => {
            let mut p = kurbo::Ellipse::from_rect(kurbo::Rect::new(x0, y0, x1, y1 - h * 0.25)).to_path(0.1);
            p.move_to((x0 + w * 0.4, y1 - h * 0.25));
            p.line_to((x0 + w * 0.2, y1));
            p.line_to((x0 + w * 0.3, y1 - h * 0.25));
            p
        }
        ShapeKind::LineCallout1
        | ShapeKind::LineCallout2
        | ShapeKind::LineCallout3
        | ShapeKind::LineCallout4
        | ShapeKind::LineCallout1AccentBar
        | ShapeKind::LineCallout2AccentBar
        | ShapeKind::LineCallout3AccentBar
        | ShapeKind::LineCallout4AccentBar => {
            let mut p = BezPath::new();
            p.move_to((x0, y1));
            p.line_to((x0 + w * 0.3, cy));
            p.line_to((x1, cy));
            if matches!(
                kind,
                ShapeKind::LineCallout1AccentBar
                    | ShapeKind::LineCallout2AccentBar
                    | ShapeKind::LineCallout3AccentBar
                    | ShapeKind::LineCallout4AccentBar
            ) {
                p.move_to((x0 + w * 0.3, y0));
                p.line_to((x0 + w * 0.3, y1));
            }
            p
        }
    }
}

/// Flattened subpath contours of a shape inside a `(0, 0, w, h)` box, suitable for UI preview rendering.
/// Returns a list of `(closed, points)` for each contour.
pub fn shape_preview_contours(kind: ShapeKind, w: f32, h: f32) -> Vec<(bool, Vec<(f32, f32)>)> {
    let r = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
    let path = shape_path(kind, r);
    let mut contours = Vec::new();
    let mut current = Vec::new();
    let mut is_closed = false;
    kurbo::flatten(path, 0.5, |el| match el {
        kurbo::PathEl::MoveTo(p) => {
            if !current.is_empty() {
                contours.push((is_closed, std::mem::take(&mut current)));
                is_closed = false;
            }
            current.push((p.x as f32, p.y as f32));
        }
        kurbo::PathEl::LineTo(p) => {
            current.push((p.x as f32, p.y as f32));
        }
        kurbo::PathEl::ClosePath => {
            is_closed = true;
            if !current.is_empty() {
                contours.push((true, std::mem::take(&mut current)));
                is_closed = false;
            }
        }
        _ => {}
    });
    if !current.is_empty() {
        contours.push((is_closed, current));
    }
    contours
}

/// Render the page area (`x`, `y`, `w`×`h` points) at `scale` px/pt — for previews and thumbnails.
pub fn render_area(doc: &Document, page: &Page, x: f32, y: f32, w: f32, h: f32, scale: f32, opts: &RenderOptions) -> Rendered {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let pw = ((w * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let ph = ((h * scale).ceil() as u32).clamp(1, MAX_SIDE);
    let view = Affine::scale(scale as f64) * Affine::translate((-x as f64, -y as f64));
    render_region(doc, page, pw, ph, view, opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_layout::{LayoutCache, LayoutOptions, layout};

    #[test]
    fn renders_text_dark_pixels() {
        let d = Document::from_text("Hello WordCraft");
        let l = layout(&d, &mut LayoutCache::new(), &LayoutOptions::default());
        let img = render_page(&d, &l.pages[0], 1.0, &RenderOptions::default());
        assert_eq!((img.width, img.height), (612, 792));
        // Somewhere in the first text line there are dark pixels; the margins are white.
        let dark = (72..300).flat_map(|x| (72..100).map(move |y| (x, y))).filter(|(x, y)| img.pixel(*x, *y)[0] < 128).count();
        assert!(dark > 30, "dark {dark}");
        assert_eq!(img.pixel(5, 5), [255, 255, 255, 255]);
        assert!(!img.to_png().is_empty());
    }

    #[test]
    fn hostile_scale_is_clamped() {
        let d = Document::new();
        let l = layout(&d, &mut LayoutCache::new(), &LayoutOptions::default());
        let img = render_page(&d, &l.pages[0], f32::NAN, &RenderOptions::default());
        assert_eq!(img.width, 612);
        let big = render_page(&d, &l.pages[0], 1e9, &RenderOptions::default());
        assert!(big.width <= MAX_SIDE);
        assert!(decode_pixmap(b"not an image").is_none());
    }

    #[test]
    fn dimmed_header_image_renders() {
        let mut d = Document::from_text("body");
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let media = d.add_media(png, "png");
        let mut hp = wordcraft_doc::Paragraph::new();
        let obj = wordcraft_doc::para::InlineObject::Image { media, w: 40.0, h: 40.0, alt: String::new(), float: Default::default(), crop: [0.0; 4] };
        hp.insert_object(0, obj, &Default::default()).unwrap();
        let id = d.add_part(wordcraft_doc::PartKind::Header, vec![wordcraft_doc::para_block(hp)]);
        d.last_section.headers.default = Some(id);
        let l = layout(&d, &mut LayoutCache::new(), &LayoutOptions::default());
        let opts = RenderOptions::default();
        assert!(opts.display.dim_header);
        let img = render_page(&d, &l.pages[0], 1.0, &opts);
        assert!(!img.to_png().is_empty());
    }

    #[test]
    fn shapes_have_paths() {
        let r = kurbo::Rect::new(0.0, 0.0, 10.0, 10.0);
        for k in [
            ShapeKind::Rectangle,
            ShapeKind::RoundedRectangle,
            ShapeKind::SingleCornerSnipped,
            ShapeKind::TopCornersSnipped,
            ShapeKind::DiagonalCornersSnipped,
            ShapeKind::SnippedAndRounded,
            ShapeKind::RoundSingleCorner,
            ShapeKind::RoundSameSideCorner,
            ShapeKind::RoundDiagonalCorner,
            ShapeKind::Ellipse,
            ShapeKind::Triangle,
            ShapeKind::RightTriangle,
            ShapeKind::Parallelogram,
            ShapeKind::Trapezoid,
            ShapeKind::Diamond,
            ShapeKind::Pentagon,
            ShapeKind::Hexagon,
            ShapeKind::Heptagon,
            ShapeKind::Octagon,
            ShapeKind::Decagon,
            ShapeKind::Dodecagon,
            ShapeKind::Pie,
            ShapeKind::Chord,
            ShapeKind::Teardrop,
            ShapeKind::Frame,
            ShapeKind::HalfFrame,
            ShapeKind::LShape,
            ShapeKind::DiagonalStripe,
            ShapeKind::Cross,
            ShapeKind::Plaque,
            ShapeKind::Cylinder,
            ShapeKind::Cube,
            ShapeKind::Bevel,
            ShapeKind::Donut,
            ShapeKind::NoSymbol,
            ShapeKind::BlockArc,
            ShapeKind::FoldedCorner,
            ShapeKind::SmileyFace,
            ShapeKind::Heart,
            ShapeKind::LightningBolt,
            ShapeKind::Sun,
            ShapeKind::Moon,
            ShapeKind::Cloud,
            ShapeKind::Arc,
            ShapeKind::DoubleBracket,
            ShapeKind::DoubleBrace,
            ShapeKind::LeftBracket,
            ShapeKind::RightBracket,
            ShapeKind::LeftBrace,
            ShapeKind::RightBrace,
            ShapeKind::TextBox,
            ShapeKind::Line,
            ShapeKind::Arrow,
            ShapeKind::DoubleArrow,
            ShapeKind::ElbowConnector,
            ShapeKind::ElbowArrow,
            ShapeKind::ElbowDoubleArrow,
            ShapeKind::CurvedConnector,
            ShapeKind::CurvedArrow,
            ShapeKind::CurvedDoubleArrow,
            ShapeKind::Curve,
            ShapeKind::Freeform,
            ShapeKind::Scribble,
            ShapeKind::LeftArrow,
            ShapeKind::UpArrow,
            ShapeKind::DownArrow,
            ShapeKind::LeftRightArrow,
            ShapeKind::UpDownArrow,
            ShapeKind::QuadArrow,
            ShapeKind::LeftRightUpArrow,
            ShapeKind::BentArrow,
            ShapeKind::UTurnArrow,
            ShapeKind::LeftUpArrow,
            ShapeKind::BentUpArrow,
            ShapeKind::StripedRightArrow,
            ShapeKind::NotchedRightArrow,
            ShapeKind::PentagonArrow,
            ShapeKind::Chevron,
            ShapeKind::RightArrowCallout,
            ShapeKind::DownArrowCallout,
            ShapeKind::LeftArrowCallout,
            ShapeKind::UpArrowCallout,
            ShapeKind::LeftRightArrowCallout,
            ShapeKind::QuadArrowCallout,
            ShapeKind::CircularArrow,
            ShapeKind::MathPlus,
            ShapeKind::MathMinus,
            ShapeKind::MathMultiply,
            ShapeKind::MathDivide,
            ShapeKind::MathEqual,
            ShapeKind::MathNotEqual,
            ShapeKind::FlowchartProcess,
            ShapeKind::FlowchartAlternateProcess,
            ShapeKind::FlowchartDecision,
            ShapeKind::FlowchartData,
            ShapeKind::FlowchartPredefinedProcess,
            ShapeKind::FlowchartInternalStorage,
            ShapeKind::FlowchartDocument,
            ShapeKind::FlowchartMultidocument,
            ShapeKind::FlowchartTerminator,
            ShapeKind::FlowchartPreparation,
            ShapeKind::FlowchartManualInput,
            ShapeKind::FlowchartManualOperation,
            ShapeKind::FlowchartConnector,
            ShapeKind::FlowchartOffpageConnector,
            ShapeKind::FlowchartCard,
            ShapeKind::FlowchartPunchedTape,
            ShapeKind::FlowchartSummingJunction,
            ShapeKind::FlowchartOr,
            ShapeKind::FlowchartCollate,
            ShapeKind::FlowchartSort,
            ShapeKind::FlowchartExtract,
            ShapeKind::FlowchartMerge,
            ShapeKind::FlowchartStoredData,
            ShapeKind::FlowchartDelay,
            ShapeKind::FlowchartSequentialData,
            ShapeKind::FlowchartMagneticDisk,
            ShapeKind::FlowchartDirectAccessStorage,
            ShapeKind::FlowchartDisplay,
            ShapeKind::Explosion1,
            ShapeKind::Explosion2,
            ShapeKind::Star4,
            ShapeKind::Star,
            ShapeKind::Star6,
            ShapeKind::Star7,
            ShapeKind::Star8,
            ShapeKind::Star10,
            ShapeKind::Star12,
            ShapeKind::Star16,
            ShapeKind::Star24,
            ShapeKind::Star32,
            ShapeKind::UpRibbon,
            ShapeKind::DownRibbon,
            ShapeKind::CurvedUpRibbon,
            ShapeKind::CurvedDownRibbon,
            ShapeKind::VerticalScroll,
            ShapeKind::HorizontalScroll,
            ShapeKind::Wave,
            ShapeKind::DoubleWave,
            ShapeKind::RectangularCallout,
            ShapeKind::RoundedRectangularCallout,
            ShapeKind::OvalCallout,
            ShapeKind::CloudCallout,
            ShapeKind::LineCallout1,
            ShapeKind::LineCallout2,
            ShapeKind::LineCallout3,
            ShapeKind::LineCallout4,
            ShapeKind::LineCallout1AccentBar,
            ShapeKind::LineCallout2AccentBar,
            ShapeKind::LineCallout3AccentBar,
            ShapeKind::LineCallout4AccentBar,
            ShapeKind::DrawingCanvas,
        ] {
            assert!(!shape_path(k, r).elements().is_empty(), "{k:?}");
        }
    }
}
