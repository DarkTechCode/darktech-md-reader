//! Регрессия «дня чтения»: оглавление на стороне «Справа» рисовало ПУСТУЮ
//! колонку. Причина: внутри ScrollArea::show_viewport painter/interact работают
//! в ЭКРАННЫХ координатах, а outline::show красил строки в КОНТЕНТНЫХ — у
//! левой панели при нулевом отступе прокрутки они совпадают, справа строки
//! ложились за клипом панели и вырезались. Тест: реальный outline::show в
//! последовательности панелей app.rs; после смены стороны (и при прокрутке
//! списка колесом) текстовые шейпы обязаны лежать ВНУТРИ прямоугольника
//! панели оглавления.

use std::sync::Arc;

use darktech_md::fonts;
use darktech_md::i18n::Lang;
use darktech_md::layout::{LayoutKey, LayoutState};
use darktech_md::md::parser::parse;
use darktech_md::settings::model::TextAlign;
use darktech_md::view::themes::{self, Typo};
use darktech_md::view::{outline, settings_panel};
use eframe::egui;

fn doc_src() -> String {
    let mut s = String::new();
    for i in 0..12 {
        s.push_str(&format!("# Раздел {i}\n\ntекст раздела {i}\n\n"));
        for j in 0..3 {
            s.push_str(&format!("### Подраздел {i}.{j}\n\ntекст подраздела\n\n"));
        }
    }
    s
}

struct FrameOut {
    outline_rect: egui::Rect,
    /// Текстовые шейпы внутри прямоугольника панели оглавления.
    text_in_panel: usize,
}

fn frame(
    ctx: &egui::Context,
    input: &egui::RawInput,
    side_right: bool,
    settings_open: bool,
    doc: &darktech_md::md::blocks::Doc,
    layout: &mut LayoutState,
    toc: &mut outline::TocState,
    settings: &mut darktech_md::settings::model::SettingsData,
) -> FrameOut {
    let typo = Typo::default();
    layout.ensure(
        doc,
        LayoutKey {
            typo: typo.clone(),
            width: typo.width,
            dark: true,
            lang: Lang::Ru,
            code_nums: false,
            table_scroll: true,
            text_align: TextAlign::Left,
        },
    );
    let pal = &themes::DARK;
    let mut outline_rect = egui::Rect::NOTHING;

    let mut out = ctx.run_ui(input.clone(), |ui| {
        if settings_open {
            egui::Panel::right("settings_panel")
                .resizable(false)
                .exact_size(430.0)
                .frame(egui::Frame::NONE.fill(pal.code_bg))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("settings_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            settings_panel::show(ui, settings, pal, Lang::Ru);
                        });
                });
        }
        let panel = if side_right {
            egui::Panel::right("outline")
        } else {
            egui::Panel::left("outline")
        };
        panel
            .resizable(false)
            .exact_size(260.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(pal.bg))
            .show(ui, |ui| {
                outline::show(ui, doc, layout, pal, 7, 6, 14.0, toc);
                outline_rect = ui.max_rect();
            });
        egui::Panel::right("minimap")
            .resizable(false)
            .exact_size(80.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(pal.bg))
            .show(ui, |_ui| {});
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |_ui| {});
    });
    let text_in_panel = out
        .shapes
        .iter()
        .filter(|s| match &s.shape {
            eframe::epaint::Shape::Text(t) => {
                let r = t.galley.rect.translate(t.pos.to_vec2());
                outline_rect.intersects(r)
            }
            _ => false,
        })
        .count();
    out.shapes.clear();
    out.textures_delta.clear();
    FrameOut {
        outline_rect,
        text_in_panel,
    }
}

/// Сценарий владельца: слева → открыл настройки → переключил «справа» →
/// закрыл настройки; затем прокрутка списка колесом на правой стороне.
#[test]
fn toc_side_right_paints_rows_inside_panel() {
    let ctx = egui::Context::default();
    fonts::install(&ctx);

    let doc = parse(Arc::from(doc_src().as_str()));
    let mut input = egui::RawInput::default();
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 0.0),
        egui::vec2(1280.0, 900.0),
    ));

    let mut layout = LayoutState::default();
    let mut toc = outline::TocState::default();
    let mut settings = darktech_md::settings::model::SettingsData::default();

    // Слева — работало (контентные координаты совпадали с экранными).
    for _ in 0..2 {
        let f = frame(&ctx, &input, false, false, &doc, &mut layout, &mut toc, &mut settings);
        assert!(f.text_in_panel > 0, "слева: строки оглавления на месте");
    }
    // Открыли настройки, переключили сторону на «справа» — прежний баг:
    // панель занимает правильный rect, но 0 шейпов внутри.
    let f = frame(&ctx, &input, false, true, &doc, &mut layout, &mut toc, &mut settings);
    assert!(f.text_in_panel > 0, "слева+настройки");
    let f = frame(&ctx, &input, true, true, &doc, &mut layout, &mut toc, &mut settings);
    assert!(
        f.text_in_panel > 0,
        "справа+настройки: {} шейпов в {:?}",
        f.text_in_panel,
        f.outline_rect
    );
    // Настройки закрыли — панель у правого края окна.
    let f = frame(&ctx, &input, true, false, &doc, &mut layout, &mut toc, &mut settings);
    assert!(f.text_in_panel > 0, "справа: строки внутри панели");
    assert!(
        (f.outline_rect.max.x - 1280.0).abs() < 1.0,
        "панель у правого края: {:?}",
        f.outline_rect
    );

    // Прокрутка списка колесом над панелью: offset уходит от нуля — строки
    // обязаны оставаться внутри прямоугольника панели (экран = origin + y).
    for side_right in [true, false] {
        let mut scrolled = input.clone();
        let panel_x = if side_right { 1280.0 - 260.0 - 40.0 } else { 130.0 };
        scrolled.events = vec![
            egui::Event::PointerMoved(egui::pos2(panel_x, 450.0)),
            egui::Event::MouseWheel {
                delta: egui::vec2(0.0, -3.0),
                unit: egui::MouseWheelUnit::Line,
                modifiers: Default::default(),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            },
        ];
        // Несколько кадров: скролл-физика egui применяет offset.
        let mut f = frame(&ctx, &scrolled, side_right, false, &doc, &mut layout, &mut toc, &mut settings);
        for _ in 0..3 {
            f = frame(&ctx, &input, side_right, false, &doc, &mut layout, &mut toc, &mut settings);
        }
        assert!(
            f.text_in_panel > 0,
            "после прокрутки списка (side_right={side_right}): строки внутри панели"
        );
    }
}
