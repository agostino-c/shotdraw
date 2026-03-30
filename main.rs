use eframe::egui::{self, Pos2, Rect, Stroke, Color32, CentralPanel};
use std::process::{Command, Stdio};
use std::io::{Read, Write, Cursor};
use image::{ImageBuffer, Rgba, DynamicImage, ImageFormat};

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
    };

    Command::new("swaymsg")
        .arg("for_window [title=\"Screenshot Annotator\"] floating enable, fullscreen enable")
        .output()
        .expect("Failed to run swaymsg");

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
}

struct ScreenshotApp {
    screenshot: ImageBuffer<Rgba<u8>, Vec<u8>>,
    start_pos: Option<Pos2>,
    end_pos: Option<Pos2>,
    drawing: bool,
    done: bool,
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

            // Mouse drawing
            if ctx.input(|i| i.pointer.primary_down()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    if !self.drawing {
                        self.start_pos = Some(pos);
                        self.drawing = true;
                    }
                    self.end_pos = Some(pos);
                }
            } else if self.drawing {
                self.drawing = false;
            }

            // Draw rectangle
            if let (Some(start), Some(end)) = (self.start_pos, self.end_pos) {
                ui.painter().rect_stroke(
                    Rect::from_two_pos(start, end),
                    egui::CornerRadius::ZERO,
                    Stroke::new(2.0, Color32::RED),
                    egui::StrokeKind::Middle,
                );
            }
        });

        // If finished, render and copy + exit
        if self.done {
            let annotated = render_to_image(&self.screenshot, self.start_pos, self.end_pos);
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
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut img = base.clone();

    if let (Some(s), Some(e)) = (start, end) {
        let (x0, y0) = (s.x as u32, s.y as u32);
        let (x1, y1) = (e.x as u32, e.y as u32);
        let (xmin, xmax) = (x0.min(x1), x0.max(x1));
        let (ymin, ymax) = (y0.min(y1), y0.max(y1));

        // Added basic bounds checking to prevent panics if drawing slightly off-screen
        for x in xmin..=xmax {
            if x < img.width() && ymin < img.height() { img.put_pixel(x, ymin, Rgba([255, 0, 0, 255])); }
            if x < img.width() && ymax < img.height() { img.put_pixel(x, ymax, Rgba([255, 0, 0, 255])); }
        }
        for y in ymin..=ymax {
            if xmin < img.width() && y < img.height() { img.put_pixel(xmin, y, Rgba([255, 0, 0, 255])); }
            if xmax < img.width() && y < img.height() { img.put_pixel(xmax, y, Rgba([255, 0, 0, 255])); }
        }
    }

    img
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
