use eframe::egui::{self, Pos2, Rect, Stroke, Color32, CentralPanel, TopBottomPanel};
use std::process::{Command, Stdio};
use std::io::{Read, Write, Cursor};
use image::{ImageBuffer, Rgba, DynamicImage, ImageFormat};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tool {
    Rectangle,
    Circle,
    Arrow,
}

#[derive(Clone)]
struct DrawnShape {
    tool: Tool,
    start: Pos2,
    end: Pos2,
    color: Color32,
    thickness: f32,
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
        shapes: Vec::new(),
        drag_start: None,
        drag_end: None,
        drawing: false,
        done: false,
        tool: Tool::Rectangle,
        color: Color32::RED,
        thickness: 2.0,
    };

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

    Command::new("swaymsg").arg("fullscreen disable").output().ok();
}

struct ScreenshotApp {
    screenshot: ImageBuffer<Rgba<u8>, Vec<u8>>,
    shapes: Vec<DrawnShape>,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    drawing: bool,
    done: bool,
    tool: Tool,
    color: Color32,
    thickness: f32,
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.done = true;
        }

        // Ctrl+Z: undo last committed shape
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z)) {
            self.shapes.pop();
        }

        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Tool:");
                ui.selectable_value(&mut self.tool, Tool::Rectangle, "⬜ Rect");
                ui.selectable_value(&mut self.tool, Tool::Circle, "⭕ Circle");
                ui.selectable_value(&mut self.tool, Tool::Arrow, "➡ Arrow");

                ui.separator();

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
                    let _ = label;
                }

                ui.separator();

                ui.label("Size:");
                ui.add(
                    egui::DragValue::new(&mut self.thickness)
                        .range(1.0..=10.0)
                        .speed(0.1)
                        .suffix("px"),
                );

                ui.separator();
                ui.label(format!("Shapes: {}  (Ctrl+Z undo)", self.shapes.len()));
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            let texture_id = ui.ctx().load_texture(
                "screenshot",
                egui::ColorImage::from_rgba_unmultiplied(
                    [self.screenshot.width() as usize, self.screenshot.height() as usize],
                    &self.screenshot,
                ),
                Default::default(),
            );
            ui.image(&texture_id);

            let primary_down = ctx.input(|i| i.pointer.primary_down());
            let interact_pos = ctx.input(|i| i.pointer.interact_pos());

            if primary_down {
                if let Some(pos) = interact_pos {
                    if ui.rect_contains_pointer(ui.min_rect()) || self.drawing {
                        if !self.drawing {
                            self.drag_start = Some(pos);
                            self.drawing = true;
                        }
                        self.drag_end = Some(pos);
                    }
                }
            } else if self.drawing {
                // Mouse released — commit the shape if we have a valid drag
                if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
                    self.shapes.push(DrawnShape {
                        tool: self.tool,
                        start,
                        end,
                        color: self.color,
                        thickness: self.thickness,
                    });
                }
                self.drag_start = None;
                self.drag_end = None;
                self.drawing = false;
            }

            let painter = ui.painter();

            // Render all committed shapes
            for shape in &self.shapes {
                paint_shape(painter, shape);
            }

            // Live preview of in-progress drag
            if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
                let preview = DrawnShape {
                    tool: self.tool,
                    start,
                    end,
                    color: self.color,
                    thickness: self.thickness,
                };
                paint_shape(painter, &preview);
            }
        });

        if self.done {
            let annotated = render_to_image(&self.screenshot, &self.shapes);
            copy_to_clipboard(&annotated);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn paint_shape(painter: &egui::Painter, shape: &DrawnShape) {
    let stroke = Stroke::new(shape.thickness, shape.color);
    match shape.tool {
        Tool::Rectangle => {
            painter.rect_stroke(
                Rect::from_two_pos(shape.start, shape.end),
                egui::CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Tool::Circle => {
            let center = egui::pos2(
                (shape.start.x + shape.end.x) / 2.0,
                (shape.start.y + shape.end.y) / 2.0,
            );
            let radii = egui::vec2(
                (shape.end.x - shape.start.x).abs() / 2.0,
                (shape.end.y - shape.start.y).abs() / 2.0,
            );
            painter.add(egui::Shape::Ellipse(egui::epaint::EllipseShape {
                center,
                radius: radii,
                fill: Color32::TRANSPARENT,
                stroke,
            }));
        }
        Tool::Arrow => {
            let vec = shape.end - shape.start;
            painter.arrow(shape.start, vec, stroke);
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
    shapes: &[DrawnShape],
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut img = base.clone();

    for shape in shapes {
        let px = Rgba([shape.color.r(), shape.color.g(), shape.color.b(), shape.color.a()]);
        let t = (shape.thickness.round() as u32).max(1);

        match shape.tool {
            Tool::Rectangle => {
                let (x0, y0) = (shape.start.x as u32, shape.start.y as u32);
                let (x1, y1) = (shape.end.x as u32, shape.end.y as u32);
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
                let cx = (shape.start.x + shape.end.x) / 2.0;
                let cy = (shape.start.y + shape.end.y) / 2.0;
                let rx = (shape.end.x - shape.start.x).abs() / 2.0;
                let ry = (shape.end.y - shape.start.y).abs() / 2.0;
                render_ellipse(&mut img, cx, cy, rx, ry, px, t);
            }
            Tool::Arrow => {
                render_arrow(&mut img, shape.start, shape.end, px, t);
            }
        }
    }

    img
}

fn render_arrow(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    start: Pos2,
    end: Pos2,
    px: Rgba<u8>,
    thickness: u32,
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let dir_x = dx / len;
    let dir_y = dy / len;
    let perp_x = -dir_y;
    let perp_y = dir_x;

    let head_len = 20.0 + thickness as f32 * 3.0;
    let head_width = 8.0 + thickness as f32 * 2.5;

    // Shaft ends before the arrowhead base so the line doesn't overdraw the triangle
    let shaft_end_x = end.x - dir_x * head_len;
    let shaft_end_y = end.y - dir_y * head_len;

    draw_line(
        img,
        start.x as i64, start.y as i64,
        shaft_end_x as i64, shaft_end_y as i64,
        px, thickness,
    );

    // Arrowhead triangle vertices
    let tip = (end.x, end.y);
    let base_center = (end.x - dir_x * head_len, end.y - dir_y * head_len);
    let left  = (base_center.0 + perp_x * head_width, base_center.1 + perp_y * head_width);
    let right = (base_center.0 - perp_x * head_width, base_center.1 - perp_y * head_width);

    fill_triangle(img, tip, left, right, px);
}

// Bresenham line with a t×t square brush at each plotted point
fn draw_line(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    x0: i64, y0: i64,
    x1: i64, y1: i64,
    px: Rgba<u8>,
    t: u32,
) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let half = (t as i64) / 2;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    let mut cx = x0;
    let mut cy = y0;

    loop {
        // Paint t×t square brush
        for ky in 0..t as i64 {
            for kx in 0..t as i64 {
                let px_x = cx - half + kx;
                let px_y = cy - half + ky;
                if px_x >= 0 && px_x < w && px_y >= 0 && px_y < h {
                    img.put_pixel(px_x as u32, px_y as u32, px);
                }
            }
        }

        if cx == x1 && cy == y1 { break; }

        let e2 = 2 * err;
        if e2 >= dy {
            if cx == x1 { break; }
            err += dy;
            cx += sx;
        }
        if e2 <= dx {
            if cy == y1 { break; }
            err += dx;
            cy += sy;
        }
    }
}

// Scanline fill for a triangle defined by three float-coordinate vertices
fn fill_triangle(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    v0: (f32, f32),
    v1: (f32, f32),
    v2: (f32, f32),
    px: Rgba<u8>,
) {
    let w = img.width() as i64;
    let h = img.height() as i64;

    let y_min = v0.1.min(v1.1).min(v2.1).floor() as i64;
    let y_max = v0.1.max(v1.1).max(v2.1).ceil()  as i64;

    let edges = [(v0, v1), (v1, v2), (v2, v0)];

    for y in y_min..=y_max {
        if y < 0 || y >= h { continue; }
        let yf = y as f32 + 0.5;

        let mut x_intersections: Vec<f32> = Vec::new();
        for &(a, b) in &edges {
            let (ay, by) = (a.1, b.1);
            // Does this scanline cross the edge?
            if (ay <= yf && by > yf) || (by <= yf && ay > yf) {
                let t = (yf - ay) / (by - ay);
                x_intersections.push(a.0 + t * (b.0 - a.0));
            }
        }

        if x_intersections.len() < 2 { continue; }
        x_intersections.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let x0 = x_intersections[0].floor() as i64;
        let x1 = x_intersections[x_intersections.len() - 1].ceil() as i64;

        for x in x0..=x1 {
            if x >= 0 && x < w {
                img.put_pixel(x as u32, y as u32, px);
            }
        }
    }
}

fn render_ellipse(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    cx: f32, cy: f32, rx: f32, ry: f32,
    px: Rgba<u8>, thickness: u32,
) {
    let w = img.width() as i64;
    let h = img.height() as i64;
    let steps = ((rx.max(ry) * 2.0 * std::f32::consts::PI * 2.0) as usize).max(1440);
    let half_t = (thickness as f32 - 1.0) / 2.0;

    for i in 0..steps {
        let angle = (i as f32 / steps as f32) * 2.0 * std::f32::consts::PI;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
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
