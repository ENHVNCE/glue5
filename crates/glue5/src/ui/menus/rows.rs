//! A menu's rows: each item is one focusable widget with the settings'
//! look (its label and note on the left, its control on the right). The
//! controls' own touch targets (arrows, a rail, a field) take taps and
//! drags but not the focus, so a controller stops on rows only.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::*;
use glue5_mods::{MenuInput, MenuItem, MenuKind};

use super::super::theme::{INK, MUTED, density};
use super::super::widgets::focus;
use super::{Landing, Level, RowAction, row_id};

pub(super) const FILL: Color32 = Color32::from_gray(28);
const FILL_LIT: Color32 = Color32::from_gray(33);

/// A row as allocated: the whole of it, where its control goes, and the
/// row's widget.
pub(super) struct Row {
    pub control: Rect,
    pub response: Response,
}

/// `text` in one line, cut with … at `width`.
pub(super) fn truncated(ui: &Ui, text: RichText, width: f32) -> Arc<Galley> {
    WidgetText::from(text).into_galley(ui, Some(TextWrapMode::Truncate), width.max(1.0), TextStyle::Body)
}

/// A row with the settings' look: `label` (and `note`, up to three lines)
/// on the left, `width` on the right for the control. `sense` is the
/// row's: click (focusable) for the rows that do something, hover for
/// those that only show.
pub(super) fn frame(
    ui: &mut Ui,
    id: Id,
    label: &str,
    note: Option<&str>,
    width: f32,
    sense: Sense,
    landing: &mut Landing,
) -> Row {
    let s = density(ui);
    let pad = 12.0 * s;
    let inner_width = ui.available_width() - 2.0 * pad;
    let width = width.min(inner_width * 0.7);
    let text_width = (inner_width - width - ui.spacing().item_spacing.x).max(1.0);
    let title = truncated(ui, RichText::new(label).size(14.0 * s).color(Color32::WHITE), text_width);
    let note = note.map(|note| {
        let mut job = LayoutJob::simple(note.into(), FontId::proportional(11.0 * s), MUTED, text_width);
        job.wrap =
            TextWrapping { max_rows: 3, break_anywhere: false, overflow_character: Some('…'), max_width: text_width };
        ui.fonts_mut(|f| f.layout_job(job))
    });
    let text_height = title.size().y + note.as_ref().map_or(0.0, |n| 2.0 * s + n.size().y);
    let height = text_height.max(44.0 * s) + 2.0 * pad;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let lit = sense.senses_click() && ui.is_enabled() && (response.hovered() || response.is_pointer_button_down_on());
    let radius = 14.0 * s;
    let painter = ui.painter();
    painter.rect_filled(rect, radius, if lit { FILL_LIT } else { FILL });
    let inner = rect.shrink(pad);
    let top = inner.center().y - text_height * 0.5;
    painter.galley(pos2(inner.left(), top), title.clone(), Color32::WHITE);
    if let Some(note) = note {
        painter.galley(pos2(inner.left(), top + title.size().y + 2.0 * s), note, MUTED);
    }
    let control = Rect::from_min_max(
        pos2(inner.right() - width, inner.center().y - 22.0 * s),
        pos2(inner.right(), inner.center().y + 22.0 * s),
    );
    if response.has_focus() {
        painter.rect_stroke(rect, radius, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
    }
    landing.row(&response);
    Row { control, response: focus(response) }
}

/// A chevron at `at` pointing `towards` (the start page's look).
pub(super) fn chevron(ui: &Ui, at: Pos2, towards: Vec2, color: Color32) {
    let s = density(ui);
    let (side, depth) = (towards.rot90() * 6.0 * s, towards * 3.5 * s);
    ui.painter().add(Shape::line(vec![at - depth + side, at + depth, at - depth - side], Stroke::new(2.0 * s, color)));
}

/// An item's row; what the player did with it.
pub(super) fn row(ui: &mut Ui, item: &MenuItem, landing: &mut Landing) -> Option<RowAction> {
    let s = density(ui);
    let id = row_id(item.id);
    let note = item.note.as_deref();
    let available = ui.available_width() - 24.0 * s;
    match &item.kind {
        MenuKind::Button { action } => {
            let action = action.as_deref().unwrap_or("Run");
            let text = ui.painter().layout_no_wrap(action.into(), FontId::proportional(13.0 * s), Color32::WHITE);
            let pill = text.size().x + 24.0 * s;
            let row = frame(ui, id, &item.label, note, pill.min(180.0 * s), Sense::click(), landing);
            let rect = Rect::from_min_max(
                pos2(row.control.left(), row.control.center().y - 20.0 * s),
                pos2(row.control.right(), row.control.center().y + 20.0 * s),
            );
            let fill = match (row.response.is_pointer_button_down_on(), row.response.hovered() && ui.is_enabled()) {
                (true, _) => 70,
                (false, true) => 60,
                (false, false) => 39,
            };
            ui.painter().rect_filled(rect, 10.0 * s, Color32::from_gray(fill));
            let text =
                truncated(ui, RichText::new(action).size(13.0 * s).color(Color32::WHITE), rect.width() - 16.0 * s);
            ui.painter().galley(rect.center() - text.size() * 0.5, text, Color32::WHITE);
            row.response.clicked().then_some(RowAction::Input(MenuInput::Press))
        }
        MenuKind::Toggle(on) => {
            let row = frame(ui, id, &item.label, note, 44.0 * s, Sense::click(), landing);
            switch(ui, row.control, id, *on);
            row.response.clicked().then_some(RowAction::Input(MenuInput::Toggle(!on)))
        }
        MenuKind::Submenu(menu) => {
            let row = frame(ui, id, &item.label, note, 16.0 * s, Sense::click(), landing);
            chevron(ui, row.control.right_center() - vec2(4.0 * s, 0.0), vec2(1.0, 0.0), MUTED);
            let keys = row.response.has_focus();
            row.response.clicked().then_some(RowAction::Push { level: Level::Menu(*menu), keys })
        }
        MenuKind::Label => {
            ui.add(Label::new(RichText::new(&item.label).size(12.0 * s).color(MUTED)).wrap());
            None
        }
        MenuKind::Separator => {
            ui.add_space(8.0 * s);
            if !item.label.is_empty() {
                ui.add(
                    Label::new(super::super::theme::heading(&item.label.to_uppercase(), 15.0 * s).color(MUTED))
                        .truncate(),
                );
            }
            None
        }
        MenuKind::Progress(done) => {
            let row = frame(ui, id, &item.label, note, (available * 0.55).min(260.0 * s), Sense::hover(), landing);
            let c = row.control;
            let text = ui.painter().layout_no_wrap(
                format!("{:.0}%", done.clamp(0.0, 1.0) * 100.0),
                FontId::proportional(14.0 * s),
                Color32::WHITE,
            );
            ui.painter().galley(
                pos2(c.right() - text.size().x, c.center().y - text.size().y * 0.5),
                text,
                Color32::WHITE,
            );
            let rail = Rect::from_min_max(
                pos2(c.left(), c.center().y - 2.0 * s),
                pos2(c.right() - 56.0 * s, c.center().y + 2.0 * s),
            );
            ui.painter().rect_filled(rail, 2.0 * s, Color32::from_gray(60));
            let filled = Rect::from_min_size(rail.min, vec2(rail.width() * done.clamp(0.0, 1.0), rail.height()));
            ui.painter().rect_filled(filled, 2.0 * s, Color32::WHITE);
            None
        }
        // Kinds this page does not show yet: their label.
        _ => {
            frame(ui, id, &item.label, note, 0.0, Sense::hover(), landing);
            None
        }
    }
}

/// The toggle's switch (as `widgets::toggle`), drawn in `rect`.
fn switch(ui: &Ui, rect: Rect, id: Id, on: bool) {
    let s = density(ui);
    let t = ui.ctx().animate_bool_responsive(id, on);
    ui.painter().rect_filled(
        Rect::from_center_size(rect.center(), vec2(40.0 * s, 24.0 * s)),
        13.0 * s,
        if on { Color32::WHITE } else { Color32::from_gray(65) },
    );
    let x = lerp((rect.left() + 14.0 * s)..=(rect.right() - 14.0 * s), t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 8.0 * s, if on { INK } else { MUTED });
}
