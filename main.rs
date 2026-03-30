use eframe::egui::{self, Pos2, Rect, Stroke, Color32, CentralPanel, TopBottomPanel, TextureHandle};
use std::process::{Command, Stdio};
use std::io::{Write, Cursor};
use std::io::Read as _;
use image::{ImageBuffer, Rgba, DynamicImage, ImageFormat};

const TOOLBAR_HEIGHT: f32 = 36.0;
const MIN_DRAG_PX: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tool {
    Rectangle,
    Circle,
    Arrow,
    Text,
}

#[derive(Clone)]
struct DrawnShape {
    tool: Tool,
    start: Pos2,
    end: Pos2,
    color: Color32,
    thickness: f32,
    text: String,
    font_size: f32,
}

fn load_system_font() -> Vec<u8> {
    if let Ok(output) = Command::new("fc-match")
        .args(["--format=%{file}", "sans:regular"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                if let Ok(data) = std::fs::read(&path) {
                    return data;
                }
            }
        }
    }
    for path in &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
    ] {
        if let Ok(data) = std::fs::read(path) {
            return data;
        }
    }
    panic!("Could not find a system font — install fonts-dejavu or similar");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("shotdraw {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let geometry = run_slurp();
    let png_bytes = run_grim(&geometry);
    let screenshot = image::load_from_memory(&png_bytes).expect("grim produced invalid image data");
    let rgba = screenshot.to_rgba8();
    let width = rgba.width() as f32;
    let height = rgba.height() as f32;

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(150));
        Command::new("swaymsg")
            .arg(r#"[title="Screenshot Annotator"] floating enable, fullscreen enable"#)
            .output()
            .ok();
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height + TOOLBAR_HEIGHT]),
        ..Default::default()
    };

    eframe::run_native(
        "Screenshot Annotator",
        native_options,
        Box::new(move |cc| {
            let texture = cc.egui_ctx.load_texture(
                "screenshot",
                egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    &rgba,
                ),
                Default::default(),
            );

            let font_bytes = load_system_font();

            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system".to_owned(),
                egui::FontData::from_owned(font_bytes.clone()).into(),
            );
            fonts.families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "system".to_owned());
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(ScreenshotApp {
                screenshot: rgba,
                texture,
                shapes: Vec::new(),
                drag_start: None,
                drag_end: None,
                drawing: false,
                done: false,
                tool: Tool::Rectangle,
                color: Color32::RED,
                thickness: 2.0,
                font_bytes,
                text_anchor: None,
                text_buffer: String::new(),
                font_size: 24.0,
            }))
        }),
    )
    .unwrap();

    Command::new("swaymsg").arg("fullscreen disable").output().ok();
}

struct ScreenshotApp {
    screenshot: ImageBuffer<Rgba<u8>, Vec<u8>>,
    texture: TextureHandle,
    shapes: Vec<DrawnShape>,
    drag_start: Option<Pos2>,
    drag_end: Option<Pos2>,
    drawing: bool,
    done: bool,
    tool: Tool,
    color: Color32,
    thickness: f32,
    font_bytes: Vec<u8>,
    text_anchor: Option<Pos2>,
    text_buffer: String,
    font_size: f32,
}

impl ScreenshotApp {
    fn commit_text(&mut self) {
        if let Some(anchor) = self.text_anchor.take() {
            if !self.text_buffer.is_empty() {
                self.shapes.push(DrawnShape {
                    tool: Tool::Text,
                    start: anchor,
                    end: anchor,
                    color: self.color,
                    thickness: 0.0,
                    text: std::mem::take(&mut self.text_buffer),
                    font_size: self.font_size,
                });
            } else {
                self.text_buffer.clear();
            }
        }
    }
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.text_anchor.is_some() {
                self.text_anchor = None;
                self.text_buffer.clear();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if self.text_anchor.is_some() {
                self.commit_text();
            } else {
                self.done = true;
            }
        }

        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z)) {
            self.shapes.pop();
        }

        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Tool:");
                ui.selectable_value(&mut self.tool, Tool::Rectangle, "⬜ Rect");
                ui.selectable_value(&mut self.tool, Tool::Circle, "⭕ Circle");
                ui.selectable_value(&mut self.tool, Tool::Arrow, "➡ Arrow");
                ui.selectable_value(&mut self.tool, Tool::Text, "✏ Text");

                if self.tool == Tool::Text {
                    ui.separator();
                    ui.label("Font:");
                    ui.add(
                        egui::DragValue::new(&mut self.font_size)
                            .range(8.0..=96.0)
                            .speed(0.5)
                            .suffix("pt"),
                    );
                }

                ui.separator();

                ui.label("Color:");
                let swatches = [
                    Color32::RED,
                    Color32::from_rgb(255, 128, 0),
                    Color32::YELLOW,
                    Color32::GREEN,
                    Color32::from_rgb(0, 112, 255),
                    Color32::from_rgb(255, 0, 255),
                    Color32::WHITE,
                    Color32::BLACK,
                ];
                for swatch_color in swatches {
                    let selected = self.color == swatch_color;
                    let size = egui::vec2(20.0, 20.0);
                    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                    if response.clicked() {
                        self.color = swatch_color;
                    }
                    let painter = ui.painter();
                    painter.rect_filled(rect, egui::CornerRadius::same(3), swatch_color);
                    if selected {
                        let luma = swatch_color.r() as f32 * 0.299
                            + swatch_color.g() as f32 * 0.587
                            + swatch_color.b() as f32 * 0.114;
                        let outline = if luma > 160.0 { Color32::from_black_alpha(200) } else { Color32::from_white_alpha(220) };
                        painter.rect_stroke(
                            rect,
                            egui::CornerRadius::same(3),
                            Stroke::new(2.0, outline),
                            egui::StrokeKind::Outside,
                        );
                    }
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
                ui.label(format!("Shapes: {}  (Ctrl+Z undo, Esc cancel)", self.shapes.len()));
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.image(&self.texture);

            let (primary_down, interact_pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos()));

            if self.tool == Tool::Text {
                let clicked = ctx.input(|i| i.pointer.primary_clicked());
                if clicked {
                    if let Some(pos) = interact_pos {
                        if ui.rect_contains_pointer(ui.min_rect()) {
                            self.commit_text();
                            self.text_anchor = Some(pos);
                        }
                    }
                }

                if self.text_anchor.is_some() {
                    ctx.input(|i| {
                        for event in &i.events {
                            match event {
                                egui::Event::Text(t) => self.text_buffer.push_str(t),
                                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                                    self.text_buffer.pop();
                                }
                                _ => {}
                            }
                        }
                    });
                }
            } else {
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
                    if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
                        if (end - start).length() >= MIN_DRAG_PX {
                            self.shapes.push(DrawnShape {
                                tool: self.tool,
                                start,
                                end,
                                color: self.color,
                                thickness: self.thickness,
                                text: String::new(),
                                font_size: 0.0,
                            });
                        }
                    }
                    self.drag_start = None;
                    self.drag_end = None;
                    self.drawing = false;
                }
            }

            let painter = ui.painter();
            for shape in &self.shapes {
                paint_shape(painter, shape);
            }
            if let (Some(start), Some(end)) = (self.drag_start, self.drag_end) {
                paint_shape(painter, &DrawnShape {
                    tool: self.tool,
                    start,
                    end,
                    color: self.color,
                    thickness: self.thickness,
                    text: String::new(),
                    font_size: 0.0,
                });
            }

            if self.tool == Tool::Text {
                if let Some(anchor) = self.text_anchor {
                    let blink_on = (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                    let display = if blink_on {
                        format!("{}|", self.text_buffer)
                    } else {
                        self.text_buffer.clone()
                    };
                    painter.text(
                        anchor,
                        egui::Align2::LEFT_TOP,
                        &display,
                        egui::FontId::proportional(self.font_size),
                        self.color,
                    );
                }
                ctx.request_repaint();
            }
        });

        if self.done {
            self.done = false;
            let annotated = render_to_image(&self.screenshot, &self.shapes, &self.font_bytes);
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
            if vec.length() >= MIN_DRAG_PX {
                painter.arrow(shape.start, vec, stroke);
            }
        }
        Tool::Text => {
            if !shape.text.is_empty() {
                painter.text(
                    shape.start,
                    egui::Align2::LEFT_TOP,
                    &shape.text,
                    egui::FontId::proportional(shape.font_size),
                    shape.color,
                );
            }
        }
    }
}

// --------------------
// Helper functions
// --------------------

fn run_slurp() -> String {
    let output = Command::new("slurp").output().expect("Failed to run slurp");
    if !output.status.success() {
        std::process::exit(0);
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
        .expect("Failed to spawn grim");

    let mut buffer = Vec::new();
    child.stdout.as_mut()
        .expect("grim stdout not captured")
        .read_to_end(&mut buffer)
        .expect("Failed to read grim output");
    child.wait().expect("grim failed");
    buffer
}

fn render_text(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    color: Rgba<u8>,
    font: &ab_glyph::FontVec,
) {
    use ab_glyph::{Font, PxScale, ScaleFont};

    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let mut cursor_x = x;
    let baseline_y = y + scaled.ascent();

    for ch in text.chars() {
        let gid = scaled.glyph_id(ch);
        let glyph = gid.with_scale_and_position(scale, ab_glyph::point(cursor_x, baseline_y));
        cursor_x += scaled.h_advance(gid);

        if let Some(og) = font.outline_glyph(glyph) {
            let bounds = og.px_bounds();
            let w = img.width() as i64;
            let h = img.height() as i64;
            og.draw(|gx, gy, cov| {
                if cov < 0.05 { return; }
                let px_x = bounds.min.x as i64 + gx as i64;
                let px_y = bounds.min.y as i64 + gy as i64;
                if px_x < 0 || px_x >= w || px_y < 0 || px_y >= h { return; }
                let base = img.get_pixel(px_x as u32, px_y as u32);
                let a = cov;
                let out = Rgba([
                    (color[0] as f32 * a + base[0] as f32 * (1.0 - a)) as u8,
                    (color[1] as f32 * a + base[1] as f32 * (1.0 - a)) as u8,
                    (color[2] as f32 * a + base[2] as f32 * (1.0 - a)) as u8,
                    255,
                ]);
                img.put_pixel(px_x as u32, px_y as u32, out);
            });
        }
    }
}

fn render_to_image(
    base: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    shapes: &[DrawnShape],
    font_bytes: &[u8],
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut img = base.clone();

    for shape in shapes {
        let px = Rgba([shape.color.r(), shape.color.g(), shape.color.b(), shape.color.a()]);
        let t = (shape.thickness.round() as u32).max(1);

        match shape.tool {
            Tool::Rectangle => {
                let (x0, y0) = (shape.start.x as i64, shape.start.y as i64);
                let (x1, y1) = (shape.end.x as i64, shape.end.y as i64);
                let (xmin, xmax) = (x0.min(x1), x0.max(x1));
                let (ymin, ymax) = (y0.min(y1), y0.max(y1));
                let w = img.width() as i64;
                let h = img.height() as i64;

                for x in xmin..=xmax {
                    if x < 0 || x >= w { continue; }
                    for k in 0..t as i64 {
                        let ya = ymin + k;
                        let yb = ymax - k;
                        if ya >= 0 && ya < h { img.put_pixel(x as u32, ya as u32, px); }
                        if yb >= 0 && yb < h && yb != ya { img.put_pixel(x as u32, yb as u32, px); }
                    }
                }
                for y in ymin..=ymax {
                    if y < 0 || y >= h { continue; }
                    for k in 0..t as i64 {
                        let xa = xmin + k;
                        let xb = xmax - k;
                        if xa >= 0 && xa < w { img.put_pixel(xa as u32, y as u32, px); }
                        if xb >= 0 && xb < w && xb != xa { img.put_pixel(xb as u32, y as u32, px); }
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
            Tool::Text => {
                if !shape.text.is_empty() {
                    let font = ab_glyph::FontVec::try_from_vec(font_bytes.to_vec())
                        .expect("Invalid font data");
                    render_text(&mut img, &shape.text, shape.start.x, shape.start.y,
                                shape.font_size, px, &font);
                }
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
    if len < 1.0 { return; }

    let dir_x = dx / len;
    let dir_y = dy / len;
    let perp_x = -dir_y;
    let perp_y = dir_x;

    let head_len   = 20.0 + thickness as f32 * 3.0;
    let head_width =  8.0 + thickness as f32 * 2.5;

    let shaft_end_x = end.x - dir_x * head_len;
    let shaft_end_y = end.y - dir_y * head_len;

    draw_line(img, start.x as i64, start.y as i64, shaft_end_x as i64, shaft_end_y as i64, px, thickness);

    let tip         = (end.x, end.y);
    let base_center = (end.x - dir_x * head_len, end.y - dir_y * head_len);
    let left        = (base_center.0 + perp_x * head_width, base_center.1 + perp_y * head_width);
    let right       = (base_center.0 - perp_x * head_width, base_center.1 - perp_y * head_width);

    fill_triangle(img, tip, left, right, px);
}

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
        if e2 >= dy { if cx == x1 { break; } err += dy; cx += sx; }
        if e2 <= dx { if cy == y1 { break; } err += dx; cy += sy; }
    }
}

fn fill_triangle(
    img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>,
    v0: (f32, f32), v1: (f32, f32), v2: (f32, f32),
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
        let mut xs: Vec<f32> = Vec::new();
        for &(a, b) in &edges {
            if (a.1 <= yf && b.1 > yf) || (b.1 <= yf && a.1 > yf) {
                let t = (yf - a.1) / (b.1 - a.1);
                xs.push(a.0 + t * (b.0 - a.0));
            }
        }
        if xs.len() < 2 { continue; }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let x0 = xs[0].floor() as i64;
        let x1 = xs[xs.len() - 1].ceil() as i64;
        for x in x0..=x1 {
            if x >= 0 && x < w { img.put_pixel(x as u32, y as u32, px); }
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
    let steps = ((rx.max(ry) * 2.0 * std::f32::consts::PI) as usize).max(1440);
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
        .expect("Failed to encode PNG");

    let mut child = Command::new("wl-copy")
        .arg("-t")
        .arg("image/png")
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to spawn wl-copy — is it installed?");

    let mut stdin = child.stdin.take().expect("wl-copy stdin not captured");
    let writer = std::thread::spawn(move || {
        stdin.write_all(&buf).ok();
    });

    child.wait().expect("wl-copy failed");
    writer.join().ok();
}
