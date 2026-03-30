use eframe::egui::{self, Pos2, Rect, Stroke, Color32, CentralPanel, TopBottomPanel};
use std::process::{Command, Stdio};
use std::io::{Read, Write, Cursor};
use image::{ImageBuffer, Rgba, DynamicImage, ImageFormat};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tool {
    Rectangle,
    Circle,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("shotdraw {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let geometry = run_slurp();
    let png_bytes = run_grim(&geometry);
    let screenshot = image::load_from_memory(&png_bytes).unwrap();
    let rgba = screenshot.to_rgba8();
    let width = rgba.width() as f32;
    let height = rgba.height() as f32;

    let app = ScreenshotApp {
        screenshot: rgba,
        start_pos: None,
        end_pos: None,
        drawing: false,
        done: false,
        tool: Tool::Rectangle,
        color: Color32::RED,
        thickness: 2.0,
    };

    // Apply float+fullscreen as a one-shot criteria command (not for_window, which is persistent).
    // Run in a background thread so it fires after the window appears.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(150));
        Command::new("swaymsg")
            .arg(r#"[title="Screenshot Annotator"] floating enable, fullscreen enable"#)
            .output()
            .ok();
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([width, height]),
        ..Default::default()
    };

    eframe::run_native(
        "Screenshot Annotator",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .unwrap();

    // Ensure the workspace fullscreen state is cleared after the window closes.
    Command::new("swaymsg").arg("fullscreen disable").output().ok();
}

struct ScreenshotApp {
    screenshot: ImageBuffer<Rgba<u8>, Vec<u8>>,
    start_pos: Option<Pos2>,
    end_pos: Option<Pos2>,
    drawing: bool,
    done: bool,
    tool: Tool,
    color: Color32,
    thickness: f32,
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Press Escape to cancel without copying
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Press Enter to finish drawing
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.done = true;
        }

        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Tool selector
                ui.label("Tool:");
                ui.selectable_value(&mut self.tool, Tool::Rectangle, "⬜ Rect");
                ui.selectable_value(&mut self.tool, Tool::Circle, "⭕ Circle");

                ui.separator();

                // Color swatches
                ui.label("Color:");
                let swatches = [
                    ("R", Color32::RED),
                    ("O", Color32::from_rgb(255, 128, 0)),
                    ("Y", Color32::YELLOW),
                    ("G", Color32::GREEN),
                    ("B", Color32::from_rgb(0, 112, 255)),
                    ("M", Color32::from_rgb(255, 0, 255)),
                    ("W", Color32::WHITE),
                    ("K", Color32::BLACK),
                ];
                for (label, swatch_color) in swatches {
                    let selected = self.color == swatch_color;
                    let size = egui::vec2(20.0, 20.0);
                    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                    if response.clicked() {
                        self.color = swatch_color;
                    }
                    let painter = ui.painter();
                    painter.rect_filled(rect, egui::CornerRadius::same(3), swatch_color);
                    if selected {
                        painter.rect_stroke(
                            rect,
                            egui::CornerRadius::same(3),
                            Stroke::new(2.0, Color32::from_white_alpha(220)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    // Invisible label for accessibility / tooltip
                    let _ = label;
                }

                ui.separator();

                // Stroke thickness
                ui.label("Size:");
                ui.add(
                    egui::DragValue::new(&mut self.thickness)
                        .range(1.0..=10.0)
                        .speed(0.1)
                        .suffix("px"),
                );
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            // Draw screenshot as background
            let texture_id = ui.ctx().load_texture(
                "screenshot",
                egui::ColorImage::from_rgba_unmultiplied(
                    [self.screenshot.width() as usize, self.screenshot.height() as usize],
                    &self.screenshot,
                ),
                Default::default(),
            );
            ui.image(&texture_id);

            // Mouse drawing — only track clicks that land in the central panel, not the toolbar
            if ctx.input(|i| i.pointer.primary_down()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    if ui.rect_contains_pointer(ui.min_rect()) || self.drawing {
                        if !self.drawing {
                            self.start_pos = Some(pos);
                            self.drawing = true;
                        }
                        self.end_pos = Some(pos);
                    }
                }
            } else if self.drawing {
                self.drawing = false;
            }

            if let (Some(start), Some(end)) = (self.start_pos, self.end_pos) {
                let stroke = Stroke::new(self.thickness, self.color);
                match self.tool {
                    Tool::Rectangle => {
                        ui.painter().rect_stroke(
                            Rect::from_two_pos(start, end),
                            egui::CornerRadius::ZERO,
                            stroke,
                            egui::StrokeKind::Middle,
                        );
                    }
                    Tool::Circle => {
                        let center = egui::pos2(
                            (start.x + end.x) / 2.0,
                            (start.y + end.y) / 2.0,
                        );
                        let radii = egui::vec2(
                            (end.x - start.x).abs() / 2.0,
                            (end.y - start.y).abs() / 2.0,
                        );
                        ui.painter().add(egui::Shape::Ellipse(egui::epaint::EllipseShape {
                            center,
                            radius: radii,
                            fill: Color32::TRANSPARENT,
                            stroke,
                        }));
                    }
                }
            }
        });

        // If finished, render and copy + exit
        if self.done {
            let annotated = render_to_image(
                &self.screenshot,
                self.start_pos,
                self.end_pos,
                self.tool,
                self.color,
                self.thickness,
            );
            copy_to_clipboard(&annotated);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

// --------------------
// Helper functions
// --------------------

fn run_slurp() -> String {
    let output = Command::new("slurp").output().expect("Failed to run slurp");
    if !output.status.success() {
        panic!("slurp returned an error");
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run_grim(geometry: &str) -> Vec<u8> {
    let mut child = Command::new("grim")
        .arg("-g")
        .arg(geometry)
        .arg("-")
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to run grim");

    let mut buffer = Vec::new();
    child.stdout.as_mut().unwrap().read_to_end(&mut buffer).unwrap();
    buffer
}

fn render_to_image(
    base: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    start: Option<Pos2>,
    end: Option<Pos2>,
    tool: Tool,
    color: Color32,
    thickness: f32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut img = base.clone();
    let px = Rgba([color.r(), color.g(), color.b(), color.a()]);
    let t = (thickness.round() as u32).max(1);

    if let (Some(s), Some(e)) = (start, end) {
        match tool {
            Tool::Rectangle => {
                let (x0, y0) = (s.x as u32, s.y as u32);
                let (x1, y1) = (e.x as u32, e.y as u32);
                let (xmin, xmax) = (x0.min(x1), x0.max(x1));
                let (ymin, ymax) = (y0.min(y1), y0.max(y1));
                let w = img.width();
                let h = img.height();

                for x in xmin..=xmax {
                    if x >= w { continue; }
                    for k in 0..t {
                        if ymin + k < h { img.put_pixel(x, ymin + k, px); }
                        if ymax >= k && ymax - k < h { img.put_pixel(x, ymax - k, px); }
                    }
                }
                for y in ymin..=ymax {
                    if y >= h { continue; }
                    for k in 0..t {
                        if xmin + k < w { img.put_pixel(xmin + k, y, px); }
                        if xmax >= k && xmax - k < w { img.put_pixel(xmax - k, y, px); }
                    }
                }
            }
            Tool::Circle => {
                let cx = (s.x + e.x) / 2.0;
                let cy = (s.y + e.y) / 2.0;
                let rx = (e.x - s.x).abs() / 2.0;
                let ry = (e.y - s.y).abs() / 2.0;
                render_ellipse(&mut img, cx, cy, rx, ry, px, t);
            }
        }
    }

    img
}

fn render_ellipse(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    cx: f32, cy: f32, rx: f32, ry: f32,
    px: Rgba<u8>, thickness: u32,
) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    // Enough steps to avoid gaps at the widest arc
    let steps = ((rx.max(ry) * 2.0 * std::f32::consts::PI * 2.0) as usize).max(1440);
    let half_t = (thickness as f32 - 1.0) / 2.0;

    for i in 0..steps {
        let angle = (i as f32 / steps as f32) * 2.0 * std::f32::consts::PI;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        // Draw `thickness` concentric ellipses offset inward/outward
        for k in 0..thickness {
            let offset = k as f32 - half_t;
            let ex = cx + (rx + offset) * cos_a;
            let ey = cy + (ry + offset) * sin_a;
            let xi = ex.round() as i64;
            let yi = ey.round() as i64;
            if xi >= 0 && xi < w && yi >= 0 && yi < h {
                img.put_pixel(xi as u32, yi as u32, px);
            }
        }
    }
}

fn copy_to_clipboard(img: &ImageBuffer<Rgba<u8>, Vec<u8>>) {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);

    DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .unwrap();

    let mut child = Command::new("wl-copy")
        .arg("-t")
        .arg("image/png")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to run wl-copy. Is it installed?");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&buf).expect("Failed to write to wl-copy stdin");
    }

    child.wait().expect("wl-copy failed to execute");
}
