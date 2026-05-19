//! Mystify screensaver for [egui](https://github.com/emilk/egui).
//!
//! Renders two bouncing quadrilaterals with colour-cycling trails onto the
//! egui background layer, recreating the classic Windows 3.x Mystify screen
//! saver.  The simulation runs at a fixed 30 fps time-step regardless of the
//! actual display refresh rate so the animation looks identical on any monitor.
//! Repaints are capped at 30 FPS; if the hardware cannot sustain that rate
//! the screensaver animates as fast as possible without any artificial delay.
//!
//! # Usage
//!
//! ```rust,no_run
//! use egui_screensaver_mystify::MystifyBackground;
//!
//! struct MyApp {
//!     mystify: MystifyBackground,
//! }
//!
//! impl eframe::App for MyApp {
//!     fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
//!         let ctx = ui.ctx().clone();
//!         // Call paint once per frame before drawing any UI windows so the
//!         // screensaver sits on the background layer behind everything else.
//!         self.mystify.paint(&ctx);
//!     }
//! }
//! ```

use std::time::Duration;

use egui::{Context, LayerId, Painter, Pos2, Shape, Stroke, Vec2, ecolor::Hsva, pos2, vec2};

// ── Simulation constants ─────────────────────────────────────────────────────

/// Maximum number of historic polygon positions kept for trail rendering.
const TRAIL_LENGTH: usize = 18;

/// Pixels of inset on every edge so vertices never touch the window border.
const EDGE_PADDING: f32 = 12.0;

/// Target simulation rate.  Physics steps always advance by `1/TARGET_FPS`
/// seconds of virtual time, decoupled from the actual rendering rate.
const TARGET_FPS: f64 = 30.0;

/// Virtual time (seconds) consumed by one simulation step.
const TARGET_FRAME_TIME: f64 = 1.0 / TARGET_FPS;

// ── Internal helpers ─────────────────────────────────────────────────────────

/// One bouncing quadrilateral and its motion trail.
#[derive(Debug)]
struct MystifyPolygon {
    /// Current vertex positions in normalised [0, 1] × [0, 1] space.
    points: Vec<Pos2>,
    /// Per-vertex velocity in normalised units per second.
    velocities: Vec<Vec2>,
    /// Ring buffer of past vertex snapshots used to draw the fading trail.
    trail: Vec<Vec<Pos2>>,
    /// Per-polygon hue phase offset so the two polygons cycle different colours.
    hue_offset: f32,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Mystify screensaver state.
///
/// Create one instance (e.g. as a field of your `eframe::App` struct) and call
/// [`MystifyBackground::paint`] every frame from your `update` method.
#[derive(Debug)]
pub struct MystifyBackground {
    /// The two animated polygons.
    polygons: Vec<MystifyPolygon>,
    /// Multiplier applied to `TARGET_FRAME_TIME` each simulation step.
    /// `0.5` means half speed; `1.0` is full speed.
    speed_factor: f32,
    /// Global hue value that cycles through [0, 1) over time.
    hue: f32,
    /// Wall-clock time (seconds) at the previous call to [`paint`].
    last_time: Option<f64>,
    /// Accumulated wall-clock time not yet consumed by simulation steps.
    time_accumulator: f64,
}

impl Default for MystifyBackground {
    fn default() -> Self {
        Self {
            polygons: vec![
                // First polygon — starts near the top-left quadrant.
                MystifyPolygon {
                    points: vec![
                        pos2(0.15, 0.20),
                        pos2(0.80, 0.14),
                        pos2(0.86, 0.78),
                        pos2(0.20, 0.84),
                    ],
                    velocities: vec![
                        vec2(0.21, 0.28),
                        vec2(-0.24, 0.19),
                        vec2(-0.18, -0.23),
                        vec2(0.26, -0.20),
                    ],
                    trail: Vec::new(),
                    hue_offset: 0.0,
                },
                // Second polygon — offset in both position and hue so the two
                // polygons are visually distinct from the start.
                MystifyPolygon {
                    points: vec![
                        pos2(0.28, 0.12),
                        pos2(0.90, 0.30),
                        pos2(0.74, 0.88),
                        pos2(0.12, 0.62),
                    ],
                    velocities: vec![
                        vec2(-0.17, 0.24),
                        vec2(-0.23, -0.18),
                        vec2(0.20, -0.22),
                        vec2(0.25, 0.16),
                    ],
                    trail: Vec::new(),
                    // 180° phase shift puts this polygon on the complementary
                    // colour relative to the first one.
                    hue_offset: 0.45,
                },
            ],
            speed_factor: 0.5,
            hue: 0.0,
            last_time: None,
            time_accumulator: 0.0,
        }
    }
}

impl MystifyBackground {
    /// Paint the screensaver onto the egui background layer for this frame.
    ///
    /// Call this once per frame **before** drawing any UI panels or windows so
    /// the animation appears behind all other content.
    ///
    /// Repaints are capped at 30 FPS; if the hardware cannot sustain that rate
    /// the screensaver animates as fast as possible without any artificial delay.
    pub fn paint(&mut self, ctx: &Context) {
        ctx.request_repaint_after(Duration::from_secs_f64(1.0 / 30.0));

        let time = ctx.input(|input| input.time);

        // Accumulate wall-clock time elapsed since the last frame, clamped to
        // 250 ms so a tab switch or debugger pause doesn't produce a huge jump.
        if let Some(last_time) = self.last_time {
            let elapsed = (time - last_time).clamp(0.0, 0.25);
            self.time_accumulator += elapsed;
        }
        self.last_time = Some(time);

        let step_dt = TARGET_FRAME_TIME as f32 * self.speed_factor;

        // Consume accumulated time in fixed-size simulation steps.
        while self.time_accumulator >= TARGET_FRAME_TIME {
            for polygon in &mut self.polygons {
                Self::step_polygon(polygon, step_dt);

                // Snapshot the current vertex positions into the trail buffer.
                polygon.trail.push(polygon.points.clone());

                // Drop the oldest snapshot once we reach capacity.
                if polygon.trail.len() > TRAIL_LENGTH {
                    polygon.trail.remove(0);
                }
            }

            // Advance the global hue slowly so colours drift over time.
            self.hue = (self.hue + step_dt * 0.06).fract();
            self.time_accumulator -= TARGET_FRAME_TIME;
        }

        let rect = ctx.content_rect();
        let painter = Painter::new(ctx.clone(), LayerId::background(), rect);

        // Draw each polygon's trail as a series of polylines.  Older snapshots
        // are drawn with lower alpha so the trail fades toward the tail.
        for polygon in &self.polygons {
            for (i, points) in polygon.trail.iter().enumerate() {
                // `progress` goes from near 0 (oldest) to 1.0 (newest).
                let progress = (i + 1) as f32 / polygon.trail.len() as f32;

                // Quadratic alpha ramp so the trail fades smoothly.
                let alpha = (progress.powf(2.0) * 220.0).round() as u8;

                // Shift hue slightly along the trail for a rainbow sweep effect.
                let hue = (self.hue + polygon.hue_offset + progress * 0.12).fract();
                let color = Hsva::new(hue, 0.75, 1.0, alpha as f32 / 255.0);

                // Map normalised coordinates to screen pixels, then close the
                // polygon by repeating the first vertex at the end.
                let mut screen_points: Vec<Pos2> = points
                    .iter()
                    .map(|point| {
                        pos2(
                            rect.left()
                                + EDGE_PADDING
                                + point.x * (rect.width() - EDGE_PADDING * 2.0),
                            rect.top()
                                + EDGE_PADDING
                                + point.y * (rect.height() - EDGE_PADDING * 2.0),
                        )
                    })
                    .collect();
                if let Some(first) = screen_points.first().copied() {
                    screen_points.push(first);
                }

                painter.add(Shape::line(
                    screen_points,
                    Stroke::new(1.6, egui::Color32::from(color)),
                ));
            }
        }
    }

    /// Advance all vertices of `polygon` by one simulation step of `dt` seconds,
    /// bouncing off the [0, 1] boundary on each axis with a perfect reflection.
    fn step_polygon(polygon: &mut MystifyPolygon, dt: f32) {
        for (point, velocity) in polygon.points.iter_mut().zip(&mut polygon.velocities) {
            point.x += velocity.x * dt;
            point.y += velocity.y * dt;

            // Reflect off left / right walls.
            if point.x <= 0.0 {
                point.x = 0.0;
                velocity.x = velocity.x.abs(); // ensure positive (rightward)
            } else if point.x >= 1.0 {
                point.x = 1.0;
                velocity.x = -velocity.x.abs(); // ensure negative (leftward)
            }

            // Reflect off top / bottom walls.
            if point.y <= 0.0 {
                point.y = 0.0;
                velocity.y = velocity.y.abs(); // ensure positive (downward)
            } else if point.y >= 1.0 {
                point.y = 1.0;
                velocity.y = -velocity.y.abs(); // ensure negative (upward)
            }
        }
    }
}
