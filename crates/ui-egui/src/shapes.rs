//! Canvas object interaction: the shape tools (drag to draw), selecting the object under the
//! pointer, dragging it to move it and its handles to resize it.

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;
use wordcraft_doc::{Pos, para::ShapeKind};
use wordcraft_geom::Rect as DocRect;
use wordcraft_layout::{DocLayout, hit::object_rects};

use crate::WordApp;
use crate::theme::{Tokens, regular};

/// Handle size on screen (points).
const HANDLE: f32 = 9.0;
/// Below this the drawing gesture inserts a default-sized shape instead.
const DRAG_SLOP: f32 = 3.0;

/// Which part of an object is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    Move,
    /// Handle index: 0 nw, 1 n, 2 ne, 3 e, 4 se, 5 s, 6 sw, 7 w.
    Resize(usize),
}

/// A drag in progress.
#[derive(Clone, Debug)]
pub struct ObjDrag {
    mode: DragMode,
    pos: Pos,
    /// The object's rect (page coordinates) when the drag started.
    rect: DocRect,
    /// The pointer position (page coordinates) when the drag started.
    from: (f32, f32),
    /// Floating offset when the drag started (page-anchored floats only).
    float: Option<(f32, f32)>,
    /// The object was already floating at drag start.
    floating: bool,
    started: bool,
}

/// The selection rectangle of the object under the pointer or selected, if any.
fn selected_object(app: &WordApp, layout: &DocLayout) -> Option<(Pos, usize, DocRect)> {
    let (pos, _) = wordcraft_engine::cmd::objects::selected(&app.session)?;
    object_rects(&app.session.doc, &layout.pages).into_iter().find(|(p, _, _)| *p == pos)
}

/// The eight resize handles of a screen rect, in the order [`DragMode::Resize`] numbers them.
fn handles(sr: Rect) -> [Rect; 8] {
    let at = |c: Pos2| Rect::from_center_size(c, vec2(HANDLE, HANDLE));
    [
        at(sr.left_top()),
        at(pos2(sr.center().x, sr.top())),
        at(sr.right_top()),
        at(sr.right_center()),
        at(sr.right_bottom()),
        at(pos2(sr.center().x, sr.bottom())),
        at(sr.left_bottom()),
        at(sr.left_center()),
    ]
}

fn fmt_in(w: f32, h: f32) -> String {
    format!("{:.2}\" × {:.2}\"", w / 72.0, h / 72.0)
}

/// Draw the shape-tool preview, the selection border and the resize handles.
pub fn draw(app: &mut WordApp, ui: &mut egui::Ui, rects: &[Rect], layout: &DocLayout, scale: f32) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter_at(ui.clip_rect());
    // Drawing preview: from the press point to the pointer.
    if let Some((page, x0, y0)) = app.canvas.drawing
        && let Some(pr) = rects.get(page)
    {
        let a = pos2(pr.min.x + x0 * scale, pr.min.y + y0 * scale);
        let p = ui.input(|i| i.pointer.hover_pos()).unwrap_or(a);
        let r = Rect::from_two_pos(a, p);
        let kind = app.canvas.shape_tool.unwrap_or(ShapeKind::Rectangle);
        let (dw, dh) = (r.width() / scale, r.height() / scale);
        let big = dw.abs() > DRAG_SLOP || dh.abs() > DRAG_SLOP;
        // The shape itself, faint, at the dragged size.
        let target = Rect::from_min_size(r.min, vec2(dw * scale, dh * scale));
        let inner = target.shrink(1.0);
        if big && inner.width() > 2.0 && inner.height() > 1.0 {
            for (closed, pts) in wordcraft_render::shape_preview_contours(kind, inner.width(), inner.height()) {
                let pts: Vec<Pos2> = pts.into_iter().map(|(x, y)| pos2(inner.min.x + x, inner.min.y + y)).collect();
                let stroke = Stroke::new(1.2, t.accent);
                if closed && !kind.is_line_like() {
                    painter.add(egui::Shape::closed_line(pts, stroke));
                } else {
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
        }
        painter.rect_stroke(target, 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Outside);
        if big {
            let label = fmt_in(dw.abs(), dh.abs());
            let fs = 11.0;
            let g = painter.layout(label.clone(), regular(fs), Color32::WHITE, 200.0);
            let box_r = Rect::from_center_size(pos2(target.center().x, target.min.y - 10.0), vec2(g.size().x + 10.0, g.size().y + 4.0));
            painter.rect_filled(box_r, 2.0, t.accent);
            painter.galley(pos2(box_r.center().x - g.size().x / 2.0, box_r.center().y - g.size().y / 2.0), g, Color32::WHITE);
        }
    }
    // Selection: border and handles around the selected object.
    if let Some((_, _, rect)) = selected_object(app, layout) {
        for (pi, pr) in rects.iter().enumerate() {
            if !pr.intersects(ui.clip_rect().expand(200.0)) {
                continue;
            }
            let _ = pi;
            let sr = Rect::from_min_size(
                pos2(pr.min.x + rect.x * scale, pr.min.y + rect.y * scale),
                vec2((rect.w * scale).max(2.0), (rect.h * scale).max(2.0)),
            );
            if !sr.intersects(ui.clip_rect().expand(60.0)) {
                continue;
            }
            let border = Stroke::new(1.25, t.accent);
            painter.rect_stroke(sr, 0.0, border, egui::StrokeKind::Outside);
            let line_like = matches!(
                wordcraft_engine::cmd::objects::selected(&app.session),
                Some((_, wordcraft_doc::para::InlineObject::Shape { kind, .. })) if kind.is_line_like()
            );
            for (i, h) in handles(sr).iter().enumerate() {
                // Line-like shapes get handles at the ends only.
                if line_like && !matches!(i, 3 | 7) {
                    continue;
                }
                painter.rect_filled(*h, 1.5, Color32::WHITE);
                painter.rect_stroke(*h, 1.5, border, egui::StrokeKind::Outside);
            }
        }
    }
}

/// Object and tool interaction. Returns true when the pointer is doing object work and the
/// text-editing mouse handler should stand aside.
pub fn interact(app: &mut WordApp, ui: &egui::Ui, resp: &egui::Response, rects: &[Rect], layout: &DocLayout, scale: f32) -> bool {
    app.canvas.obj_cursor = None;
    let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
    // ---- Shape tool armed: drag out a shape. ----
    if app.canvas.shape_tool.is_some() {
        app.canvas.obj_cursor = Some(egui::CursorIcon::Crosshair);
        let Some(p) = pointer else { return app.canvas.drawing.is_some() };
        let Some((page, x, y)) = crate::canvas::page_at(rects, scale, p) else { return app.canvas.drawing.is_some() };
        let down = ui.input(|i| i.pointer.primary_down());
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if pressed && !app.canvas.drawing.is_some() {
            app.canvas.drawing = Some((page, x, y));
            return true;
        }
        if app.canvas.drawing.is_some() {
            if down {
                return true;
            }
            // Released: insert the shape.
            if let Some((sp, sx, sy)) = app.canvas.drawing.take() {
                let (w, h) = ((x - sx).abs(), (y - sy).abs());
                let kind = app.canvas.shape_tool.unwrap_or(ShapeKind::Rectangle);
                let (w, h) = if w < DRAG_SLOP && h < DRAG_SLOP {
                    if kind.is_line_like() { (108.0, 1.0) } else { (108.0, 72.0) }
                } else {
                    (w.clamp(1.0, 2000.0), h.clamp(1.0, 2000.0))
                };
                // A drag upwards or leftwards puts the origin at the far corner.
                let (ox, oy) = ((sx.min(x)).max(0.0), (sy.min(y)).max(0.0));
                let _ = sp;
                let _ = page;
                let _ = app.run("insert.shape", json!({"kind": kind.ooxml_prst(), "width": w, "height": h, "x": ox, "y": oy, "anchor": "page"}));
            }
            return true;
        }
        return true;
    }
    // ---- Handles of the selected object: start a resize drag. ----
    if let Some((sel_pos, _, rect)) = selected_object(app, layout) {
        for pr in rects.iter() {
            if !pr.intersects(ui.clip_rect().expand(200.0)) {
                continue;
            }
            let sr = Rect::from_min_size(
                pos2(pr.min.x + rect.x * scale, pr.min.y + rect.y * scale),
                vec2((rect.w * scale).max(2.0), (rect.h * scale).max(2.0)),
            );
            for (i, h) in handles(sr).iter().enumerate() {
                let r = ui.interact(*h, ui.id().with(("obj_handle", i)), Sense::click_and_drag());
                if r.hovered() {
                    app.canvas.obj_cursor = Some(match i {
                        0 | 4 => egui::CursorIcon::ResizeNwSe,
                        2 | 6 => egui::CursorIcon::ResizeNeSw,
                        1 | 5 => egui::CursorIcon::ResizeVertical,
                        _ => egui::CursorIcon::ResizeHorizontal,
                    });
                }
                if r.drag_started() {
                    app.canvas.obj_drag = Some(ObjDrag {
                        mode: DragMode::Resize(i),
                        pos: sel_pos.clone(),
                        rect,
                        from: pointer_to_page(ui, rects, scale).unwrap_or((0.0, 0.0)),
                        float: float_offset(&app.session, &sel_pos),
                        floating: is_floating(&app.session, &sel_pos),
                        started: false,
                    });
                }
            }
        }
    }
    // ---- A drag in progress: apply while the button is down, end when it is let go. ----
    if app.canvas.obj_drag.is_some() {
        if ui.input(|i| i.pointer.primary_down()) {
            if let Some(d) = app.canvas.obj_drag.clone() {
                apply_drag(app, ui, &d);
            }
            return true;
        }
        app.canvas.obj_drag = None;
        return false;
    }
    // ---- Press on an object: select it, and drag it to move. ----
    if let Some(p) = pointer
        && let Some((page, x, y)) = crate::canvas::page_at(rects, scale, p)
    {
        let objs = object_rects(&app.session.doc, &layout.pages);
        let hit = objs.into_iter().find(|(_, opi, r)| *opi == page && r.contains(wordcraft_geom::Point::new(x, y)));
        if let Some((opos, _, orect)) = hit {
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            let already = wordcraft_engine::cmd::objects::selected(&app.session).is_some_and(|(sp, _)| sp == opos);
            if pressed {
                // Select the object by covering its character.
                let end = Pos { off: opos.off + wordcraft_doc::para::OBJ.len_utf8(), ..opos.clone() };
                if !already {
                    app.session.sel = wordcraft_engine::Selection { anchor: opos.clone(), focus: end };
                }
                app.canvas.obj_drag = Some(ObjDrag {
                    mode: DragMode::Move,
                    pos: opos.clone(),
                    rect: orect,
                    from: (x, y),
                    float: float_offset(&app.session, &opos),
                    floating: is_floating(&app.session, &opos),
                    started: false,
                });
                return true;
            }
            app.canvas.obj_cursor = if already { Some(egui::CursorIcon::Move) } else { Some(egui::CursorIcon::PointingHand) };
        }
    }
    false
}

fn pointer_to_page(ui: &egui::Ui, rects: &[Rect], scale: f32) -> Option<(f32, f32)> {
    let p = ui.input(|i| i.pointer.hover_pos())?;
    let (_, x, y) = crate::canvas::page_at(rects, scale, p)?;
    Some((x, y))
}

fn float_offset(s: &wordcraft_engine::Session, pos: &Pos) -> Option<(f32, f32)> {
    let para = s.doc.para(pos.story, &pos.path)?;
    match para.object_at(pos.off)? {
        wordcraft_doc::para::InlineObject::Shape { float, .. } | wordcraft_doc::para::InlineObject::Image { float, .. } => Some((float.x, float.y)),
        _ => None,
    }
}

fn is_floating(s: &wordcraft_engine::Session, pos: &Pos) -> bool {
    s.doc.para(pos.story, &pos.path).and_then(|p| p.object_at(pos.off)).is_some_and(|o| o.is_floating())
}

fn anchor_of(s: &wordcraft_engine::Session, pos: &Pos) -> &'static str {
    let a = s.doc.para(pos.story, &pos.path).and_then(|p| p.object_at(pos.off)).map(|o| match o {
        wordcraft_doc::para::InlineObject::Shape { float, .. } | wordcraft_doc::para::InlineObject::Image { float, .. } => float.h_rel,
        _ => wordcraft_doc::para::Anchor::Margin,
    });
    match a {
        Some(wordcraft_doc::para::Anchor::Page) => "page",
        Some(wordcraft_doc::para::Anchor::Paragraph) => "paragraph",
        _ => "margin",
    }
}

/// Apply the current drag to the document (one command call per frame; the first joins undo).
fn apply_drag(app: &mut WordApp, ui: &egui::Ui, d: &ObjDrag) {
    let Some((x, y)) = pointer_to_page(ui, &app.canvas.page_rects, app.canvas.scale) else { return };
    let (dx, dy) = (x - d.from.0, y - d.from.1);
    if !d.started {
        app.session.join_next_undo();
    }
    match d.mode {
        DragMode::Move => {
            if !d.floating {
                // Dragging an inline shape lifts it off the text, anchored where it was.
                let _ = app.run("arrange.position", json!({"x": d.rect.x, "y": d.rect.y, "anchor": "page"}));
            } else {
                let (bx, by) = d.float.unwrap_or((d.rect.x, d.rect.y));
                let anchor = anchor_of(&app.session, &d.pos);
                let _ = app.run("arrange.position", json!({"x": bx + dx, "y": by + dy, "anchor": anchor}));
            }
        }
        DragMode::Resize(i) => {
            let (mut w, mut h) = (d.rect.w, d.rect.h);
            let east = matches!(i, 2..=4);
            let west = matches!(i, 0 | 6 | 7);
            let south = matches!(i, 4..=6);
            let north = matches!(i, 0..=2);
            if east {
                w = d.rect.w + dx;
            }
            if west {
                w = d.rect.w - dx;
            }
            if south {
                h = d.rect.h + dy;
            }
            if north {
                h = d.rect.h - dy;
            }
            let (w, h) = (w.clamp(1.0, 2000.0), h.clamp(1.0, 2000.0));
            // Corner handles keep the aspect ratio; edges scale freely.
            let corner = (east || west) && (south || north);
            let mut params = json!({"width": w, "height": h, "lockAspect": corner});
            if corner && d.rect.w > 0.0 {
                let ratio = d.rect.h / d.rect.w;
                let (w2, h2) = if (w * ratio - h).abs() > (h / ratio - w).abs() { (w, w * ratio) } else { (h / ratio, h) };
                params["width"] = json!(w2.clamp(1.0, 2000.0));
                params["height"] = json!(h2.clamp(1.0, 2000.0));
            }
            // Resizing from the top or left edge also moves the shape.
            if d.floating && (west || north) {
                let (bx, by) = d.float.unwrap_or((d.rect.x, d.rect.y));
                let nw = params.get("width").and_then(|v| v.as_f64()).unwrap_or(w as f64) as f32;
                let nh = params.get("height").and_then(|v| v.as_f64()).unwrap_or(h as f64) as f32;
                let anchor = anchor_of(&app.session, &d.pos);
                let _ = app.run(
                    "arrange.position",
                    json!({
                        "x": if west { bx + (d.rect.w - nw) } else { bx },
                        "y": if north { by + (d.rect.h - nh) } else { by },
                        "anchor": anchor,
                    }),
                );
            }
            let _ = app.run("picture.size", params);
        }
    }
}

/// Show the size of the object being resized (Word shows it live).
pub fn size_tip(app: &WordApp, ui: &egui::Ui) {
    let Some(d) = &app.canvas.obj_drag else { return };
    if !matches!(d.mode, DragMode::Resize(_)) {
        return;
    }
    let Some((_, obj)) = wordcraft_engine::cmd::objects::selected(&app.session) else { return };
    let (w, h) = match obj {
        wordcraft_doc::para::InlineObject::Shape { w, h, .. } | wordcraft_doc::para::InlineObject::Image { w, h, .. } => (w, h),
        _ => return,
    };
    let Some(p) = ui.input(|i| i.pointer.hover_pos()) else { return };
    let g = ui.painter().layout(fmt_in(w, h), regular(11.0), Color32::WHITE, 200.0);
    let box_r = Rect::from_center_size(p + vec2(0.0, -18.0), vec2(g.size().x + 10.0, g.size().y + 4.0));
    ui.painter().rect_filled(box_r, 2.0, Color32::from_rgb(0x15, 0x60, 0x82));
    ui.painter().galley(pos2(box_r.center().x - g.size().x / 2.0, box_r.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

/// The tool the ribbon armed, if any: drawn in the status bar hint.
pub fn tool_hint(kind: ShapeKind) -> String {
    format!("{} — {}", kind.label(), crate::i18n::t("Drag on the page to draw it; Esc to cancel"))
}
