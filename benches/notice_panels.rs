#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(unused_imports)]
#[path = "../src/notice_panel.rs"]
mod notice_panel;
#[path = "support/notice_previous.rs"]
mod previous;

use eframe::egui;
use std::{hint::black_box, time::Instant};

fn actions(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let _ = ui.button("Dismiss");
        let _ = ui.button(if ui.max_rect().width() < 360.0 {
            "Retry"
        } else {
            "Retry reload"
        });
        let _ = previous::copy_button(ui);
    });
}

fn frame(
    context: &egui::Context,
    text: &str,
    panels: usize,
    width: f32,
    old: bool,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, if width < 300.0 { 240.0 } else { 820.0 }),
            )),
            time: Some(context.input(|input| input.time) + 1.0 / 60.0),
            ..Default::default()
        },
        |context| {
            let budget = (context.available_rect().height() - 8.0) / panels as f32;
            for (id, message_id) in [
                ("configuration-error", "configuration-error-message"),
                ("error", "reader-error-message"),
            ]
            .into_iter()
            .take(panels)
            {
                if old {
                    // The original error_panels cloned each complete message
                    // before calling these unchanged layout/paint helpers.
                    let text = text.to_owned();
                    let (panel, height, compact) =
                        previous::notice_panel(context, id, &text, budget);
                    panel.show(context, |ui| {
                        previous::compact_spacing(ui, compact);
                        previous::message(ui, message_id, &text, height);
                        actions(ui);
                    });
                } else {
                    notice_panel::NoticePanel::new(context, id, text, budget)
                        .show(context, message_id, actions);
                }
            }
            egui::CentralPanel::default().show(context, |_| {});
        },
    )
}

fn assert_same_paint(before: &egui::FullOutput, after: &egui::FullOutput) {
    let texts = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((shape.clip_rect, text.pos, text.galley.clone())),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let before = texts(before);
    let after = texts(after);
    assert_eq!(before.len(), after.len());
    for ((clip_before, pos_before, before), (clip_after, pos_after, after)) in
        before.into_iter().zip(after)
    {
        assert_eq!(clip_before, clip_after);
        assert_eq!(pos_before, pos_after);
        assert_eq!(before.text(), after.text());
        assert_eq!(before.size(), after.size());
        assert_eq!(before.job.sections, after.job.sections);
        assert_eq!(before.rows.len(), after.rows.len());
        for (before, after) in before.rows.iter().zip(&after.rows) {
            assert_eq!(before.pos, after.pos);
            assert_eq!(before.glyphs, after.glyphs);
        }
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 500 complete egui frames/sample; original message clones and a1c9de0 layout/paint helpers vs one borrowed message/shared galley. Warmed fonts, identical text/glyphs/positions/clips verified before timing; includes widget/input/paint output creation and release, excludes PDF/GPU/clipboard/recovery actions."
    );
    for (kind, text) in [
        ("short ASCII", "Cannot open the PDF.".to_owned()),
        (
            "long ASCII",
            "PDF failure with a long diagnostic. ".repeat(1000),
        ),
        (
            "long Unicode",
            "PDF failure — 长路径 📝 configuration/文档/ ".repeat(1000),
        ),
    ] {
        for (panels, width) in [(1, 1180.0), (2, 1180.0), (2, 240.0)] {
            let contexts = [egui::Context::default(), egui::Context::default()];
            for context in &contexts {
                context.style_mut(|style| {
                    style.spacing.button_padding = egui::vec2(11.0, 6.0);
                    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
                });
            }
            for _ in 0..32 {
                let before = frame(&contexts[0], &text, panels, width, true);
                let after = frame(&contexts[1], &text, panels, width, false);
                assert_same_paint(&before, &after);
            }
            let allocations = std::array::from_fn::<_, 2, _>(|index| {
                allocation_counter::allocations(|| {
                    black_box(frame(&contexts[index], &text, panels, width, index == 0));
                })
            });
            assert!(allocations[1].0 < allocations[0].0);
            let mut times = [Vec::new(), Vec::new()];
            for pass in 0..passes {
                for order in 0..2 {
                    let index = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..500 {
                        black_box(frame(
                            &contexts[index],
                            black_box(&text),
                            panels,
                            width,
                            index == 0,
                        ));
                    }
                    times[index].push(start.elapsed().as_secs_f64() * 2000.0);
                }
            }
            let medians = times.map(|mut values| {
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            });
            println!(
                "{kind}, {} bytes/message, {panels} panels, width {width}: µs/frame {:.3} -> {:.3}; (alloc,realloc) {:?} -> {:?}",
                text.len(),
                medians[0],
                medians[1],
                allocations[0],
                allocations[1]
            );
        }
    }
}
