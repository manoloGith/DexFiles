//! The audio and video player of the preview panel. While it plays the file whose preview is
//! shown, it takes the panel's picture area; when another file is selected it docks at the
//! bottom of the panel (about a third of it), so the previews go on above. Minimal controls:
//! previous, play/pause, next, the position, the list (when there are several files) and close.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::app::{App, Msg};
use super::icons::{self, Sym};
use super::theme;
use crate::fileinfo::Category;
use crate::i18n::t;
use crate::media;
use crate::vfs::{self, Entry};

pub struct Player {
    pub list: Vec<Entry>,
    pub index: usize,
    pub show_list: bool,
    pub state: media::State,
    pub loading: bool,
    pub error: Option<String>,
    cover: Option<egui::TextureHandle>,
    cover_for: Option<usize>,
    last_poll: Instant,
    /// Where the video was drawn this frame, and what the activity was last told.
    video_rect: Option<Rect>,
    sent_rect: Option<Option<[i32; 4]>>,
    /// The track ended and the next one was started (once per ending).
    advanced: bool,
    /// Over the whole window, the controls hiding while the pointer rests.
    pub fullscreen: bool,
    last_motion: Instant,
}

impl Player {
    pub fn current(&self) -> Option<&Entry> {
        self.list.get(self.index)
    }

    fn is_video(&self) -> bool {
        self.current()
            .is_some_and(|e| Category::of(&e.ext, false) == Category::Video)
    }
}

/// Whether an entry can be played.
pub fn playable(e: &Entry) -> bool {
    !e.is_dir()
        && matches!(
            Category::of(&e.ext, false),
            Category::Audio | Category::Video
        )
}

/// Play `list[index]`, with the rest of `list` as the queue.
pub fn start(app: &mut App, list: Vec<Entry>, index: usize) {
    if list.is_empty() {
        return;
    }
    let index = index.min(list.len() - 1);
    app.player = Some(Player {
        list,
        index,
        show_list: false,
        state: media::State::default(),
        loading: true,
        error: None,
        cover: None,
        cover_for: None,
        last_poll: Instant::now(),
        video_rect: None,
        sent_rect: None,
        advanced: false,
        fullscreen: false,
        last_motion: Instant::now(),
    });
    load(app, index);
}

/// The queue for a file: the media files selected with it, or else those of its folder.
pub fn queue_for(app: &App, e: &Entry) -> (Vec<Entry>, usize) {
    let tab = app.tab(app.active_pane);
    let selected: Vec<Entry> = tab
        .selected_entries()
        .into_iter()
        .filter(playable)
        .collect();
    let list: Vec<Entry> =
        if selected.len() > 1 && selected.iter().any(|s| s.location == e.location) {
            selected
        } else {
            tab.items().filter(|x| playable(x)).cloned().collect()
        };
    let index = list
        .iter()
        .position(|x| x.location == e.location)
        .unwrap_or(0);
    if list.is_empty() {
        (vec![e.clone()], 0)
    } else {
        (list, index)
    }
}

fn load(app: &mut App, index: usize) {
    let Some(p) = app.player.as_mut() else { return };
    let Some(entry) = p.list.get(index).cloned() else {
        return;
    };
    p.index = index;
    p.loading = true;
    p.error = None;
    p.advanced = false;
    p.state = media::State::default();
    if p.cover_for != Some(index) {
        p.cover = None;
    }
    media::stop();
    let tx = app.tx.clone();
    let ctx = app.ctx.clone();
    let video = Category::of(&entry.ext, false) == Category::Video;
    let _ = std::thread::Builder::new()
        .name("dexfiles-player".into())
        .spawn(move || {
            let result = vfs::local_copy(&entry.location, &mut |_| true)
                .map_err(|e| e.to_string())
                .and_then(|path| {
                    let opened = media::open(&path, video);
                    if !video {
                        let image = crate::preview::audio(&path, 640).and_then(|(img, _)| img);
                        let _ = tx.send(Msg::PlayerCover { index, image });
                    }
                    opened
                });
            let _ = tx.send(Msg::PlayerOpened { index, result });
            ctx.request_repaint();
        });
}

pub fn opened(app: &mut App, index: usize, result: Result<(), String>) {
    let Some(p) = app.player.as_mut() else { return };
    if p.index != index {
        return;
    }
    p.loading = false;
    if let Err(e) = result {
        p.error = Some(e);
    }
}

pub fn cover(app: &mut App, index: usize, image: Option<egui::ColorImage>) {
    let ctx = app.ctx.clone();
    let Some(p) = app.player.as_mut() else { return };
    if p.index != index {
        return;
    }
    p.cover_for = Some(index);
    p.cover = image.map(|i| ctx.load_texture("player-cover", i, egui::TextureOptions::LINEAR));
}

pub fn close(app: &mut App) {
    media::stop();
    media::set_video_rect(None);
    app.player = None;
}

fn go(app: &mut App, delta: isize) {
    let Some(p) = app.player.as_ref() else { return };
    let n = p.list.len() as isize;
    let next = p.index as isize + delta;
    if (0..n).contains(&next) {
        load(app, next as usize);
    }
}

/// Each frame: read the engine's state (a few times a second), go on to the next file at the
/// end of one, keep repainting while it plays.
pub fn tick(app: &mut App) {
    let Some(p) = app.player.as_mut() else { return };
    p.video_rect = None;
    if p.last_poll.elapsed() >= Duration::from_millis(200) {
        p.last_poll = Instant::now();
        p.state = media::state();
        if let Some(e) = p.state.error.clone() {
            p.error = Some(e);
        }
    }
    let ended = p.state.ended && !p.loading && !p.advanced;
    let more = p.index + 1 < p.list.len();
    if ended {
        p.advanced = true;
        if more {
            go(app, 1);
        }
    }
    app.ctx.request_repaint_after(Duration::from_millis(250));
}

/// After the frame: tell the activity where the video is (or to hide it).
pub fn end_frame(app: &mut App, ctx: &egui::Context) {
    let covered = app.dialog.is_some() || app.quick_look.is_some() || egui::Popup::is_any_open(ctx);
    let Some(p) = app.player.as_mut() else { return };
    let ppp = ctx.pixels_per_point();
    let rect = if covered || !p.is_video() {
        None
    } else {
        p.video_rect.map(|r| {
            [
                (r.left() * ppp).round() as i32,
                (r.top() * ppp).round() as i32,
                (r.width() * ppp).round() as i32,
                (r.height() * ppp).round() as i32,
            ]
        })
    };
    if p.sent_rect != Some(rect) {
        p.sent_rect = Some(rect);
        media::set_video_rect(rect);
    }
}

fn time(ms: u64) -> String {
    crate::preview::format_duration(ms / 1000)
}

/// The player, filling `ui`'s space: the picture on top (as big as fits), then the controls,
/// then (when open) the list.
pub fn show(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(app.dark);
    let dark_bg = Color32::from_rgb(0x12, 0x12, 0x14);
    let Some(p) = app.player.as_ref() else { return };
    let title = p.current().map(|e| e.name.clone()).unwrap_or_default();
    let list_h = if p.show_list && p.list.len() > 1 {
        (ui.available_height() * 0.4).clamp(60.0, 220.0)
    } else {
        0.0
    };
    let controls_h = 62.0;
    let full = ui.available_rect_before_wrap();
    let picture = Rect::from_min_max(
        full.min,
        pos2(
            full.right(),
            (full.bottom() - controls_h - list_h).max(full.top() + 40.0),
        ),
    );
    ui.allocate_rect(full, Sense::hover());
    let painter = ui.painter_at(full);
    painter.rect_filled(picture, CornerRadius::same(8), dark_bg);

    // The picture: the video (drawn by the activity over this rectangle), or the cover.
    let mut events: Vec<Event> = Vec::new();
    let p = match app.player.as_mut() {
        Some(p) => p,
        None => return,
    };
    if p.is_video() {
        let aspect = p
            .state
            .video
            .map(|(w, h)| w as f32 / h.max(1) as f32)
            .unwrap_or(16.0 / 9.0);
        let inner = picture.shrink(4.0);
        let (w, h) = if inner.width() / inner.height() > aspect {
            (inner.height() * aspect, inner.height())
        } else {
            (inner.width(), inner.width() / aspect)
        };
        let r = Rect::from_center_size(inner.center(), vec2(w, h));
        p.video_rect = Some(r);
        if !media::available() {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                t("player_android_only"),
                FontId::proportional(12.0),
                Color32::from_gray(170),
            );
        } else if cfg!(not(target_os = "android")) {
            // The simulated engine has no picture: a placeholder where the video goes.
            painter.rect_stroke(
                r,
                4,
                Stroke::new(1.0, Color32::from_gray(80)),
                egui::StrokeKind::Inside,
            );
            icons::sym(
                &painter,
                Rect::from_center_size(r.center(), Vec2::splat(r.height().min(r.width()) * 0.3)),
                Sym::Film,
                Color32::from_gray(110),
            );
        }
    } else if let Some(tex) = &p.cover {
        let size = tex.size_vec2();
        let inner = picture.shrink(8.0);
        let scale = (inner.width() / size.x).min(inner.height() / size.y);
        let r = Rect::from_center_size(inner.center(), size * scale);
        egui::Image::from_texture(tex)
            .corner_radius(6)
            .paint_at(ui, r);
    } else {
        let side = picture.height().min(picture.width()) * 0.45;
        icons::sym(
            &painter,
            Rect::from_center_size(picture.center(), Vec2::splat(side)),
            Sym::Music,
            Color32::from_gray(120),
        );
    }
    if p.loading {
        painter.text(
            picture.left_top() + vec2(10.0, 10.0),
            Align2::LEFT_TOP,
            "…",
            FontId::proportional(16.0),
            Color32::from_gray(200),
        );
    }
    if let Some(e) = &p.error {
        painter.text(
            picture.center_bottom() - vec2(0.0, 12.0),
            Align2::CENTER_BOTTOM,
            e,
            FontId::proportional(11.0),
            Color32::from_rgb(0xff, 0x8a, 0x80),
        );
    }

    // Controls: the title and times, the position bar, the buttons.
    let c = Rect::from_min_max(
        pos2(full.left(), picture.bottom() + 6.0),
        pos2(full.right(), picture.bottom() + controls_h),
    );
    let text = ui.visuals().text_color();
    controls(
        p,
        ui,
        &painter,
        c,
        title,
        text,
        pal.weak,
        pal.separator,
        false,
        &mut events,
    );

    // The list.
    if list_h > 0.0 {
        let lr = Rect::from_min_max(pos2(full.left(), c.bottom() + 4.0), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(lr));
        egui::ScrollArea::vertical()
            .id_salt("player-list-scroll")
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, e) in p.list.iter().enumerate() {
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                    if i == p.index {
                        ui.painter()
                            .rect_filled(r, CornerRadius::same(4), pal.selection_inactive);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(r, CornerRadius::same(4), pal.hover);
                    }
                    let color = if i == p.index {
                        theme::ACCENT
                    } else {
                        ui.visuals().text_color()
                    };
                    let g = ui.painter().layout_no_wrap(
                        e.name.clone(),
                        FontId::proportional(12.0),
                        color,
                    );
                    ui.painter().with_clip_rect(r).galley(
                        pos2(r.left() + 6.0, r.center().y - g.size().y / 2.0),
                        g,
                        color,
                    );
                    if resp.clicked() {
                        events.push(Event::Jump(i));
                    }
                }
            });
    }

    apply(app, events);
}

fn apply(app: &mut App, events: Vec<Event>) {
    for ev in events {
        match ev {
            Event::PlayPause => {
                let st = app
                    .player
                    .as_ref()
                    .map(|p| p.state.clone())
                    .unwrap_or_default();
                if st.ended {
                    media::seek(0);
                    media::play();
                } else if st.playing {
                    media::pause();
                } else {
                    media::play();
                }
                if let Some(p) = app.player.as_mut() {
                    p.state.playing = !st.playing;
                    p.last_poll = Instant::now() - Duration::from_millis(150);
                }
            }
            Event::Seek(ms) => {
                media::seek(ms);
                if let Some(p) = app.player.as_mut() {
                    p.state.position_ms = ms;
                    p.state.ended = false;
                    p.advanced = false;
                }
            }
            Event::Previous => {
                // Back to the start of the file first, as players do.
                if app
                    .player
                    .as_ref()
                    .is_some_and(|p| p.state.position_ms > 3000)
                {
                    media::seek(0);
                } else {
                    go(app, -1);
                }
            }
            Event::Next => go(app, 1),
            Event::Jump(i) => load(app, i),
            Event::ToggleList => {
                if let Some(p) = app.player.as_mut() {
                    p.show_list = !p.show_list;
                }
            }
            Event::Close => close(app),
            Event::Fullscreen => {
                if let Some(p) = app.player.as_mut() {
                    p.fullscreen = !p.fullscreen;
                    p.last_motion = Instant::now();
                }
            }
        }
    }
}

/// The controls under the picture: title and times, the position bar, the buttons.
#[allow(clippy::too_many_arguments)]
fn controls(
    p: &Player,
    ui: &mut Ui,
    painter: &egui::Painter,
    c: Rect,
    title: String,
    text: Color32,
    weak: Color32,
    track: Color32,
    fullscreen: bool,
    events: &mut Vec<Event>,
) {
    let st = p.state.clone();
    let galley = painter.layout_no_wrap(title, FontId::proportional(12.5), text);
    let clip = Rect::from_min_max(c.left_top(), pos2(c.right() - 80.0, c.top() + 16.0));
    painter
        .with_clip_rect(clip)
        .galley(c.left_top(), galley, text);
    painter.text(
        pos2(c.right(), c.top()),
        Align2::RIGHT_TOP,
        format!("{} / {}", time(st.position_ms), time(st.duration_ms)),
        FontId::proportional(11.0),
        weak,
    );
    // position bar
    let bar = Rect::from_min_max(
        pos2(c.left(), c.top() + 20.0),
        pos2(c.right(), c.top() + 26.0),
    );
    let bar_resp = ui.interact(
        bar.expand2(vec2(0.0, 6.0)),
        ui.scope_id().with("player-seek"),
        Sense::click_and_drag(),
    );
    painter.rect_filled(bar.shrink2(vec2(0.0, 1.5)), CornerRadius::same(2), track);
    let frac = if st.duration_ms > 0 {
        st.position_ms as f32 / st.duration_ms as f32
    } else {
        0.0
    };
    let filled = Rect::from_min_max(
        bar.left_top() + vec2(0.0, 1.5),
        pos2(
            bar.left() + bar.width() * frac.clamp(0.0, 1.0),
            bar.bottom() - 1.5,
        ),
    );
    painter.rect_filled(filled, CornerRadius::same(2), theme::ACCENT);
    if bar_resp.hovered() || bar_resp.dragged() {
        painter.circle_filled(pos2(filled.right(), bar.center().y), 5.0, theme::ACCENT);
    }
    if (bar_resp.clicked() || bar_resp.drag_stopped())
        && st.duration_ms > 0
        && let Some(pos) = bar_resp.interact_pointer_pos()
    {
        let f = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        events.push(Event::Seek((f * st.duration_ms as f32) as u64));
    }
    // buttons, centred; list and close at the right
    let by = c.top() + 44.0;
    let size = 26.0;
    let n = p.list.len();
    let has_prev = p.index > 0;
    let has_next = p.index + 1 < n;
    let playing = st.playing;
    let centre = c.center().x;
    for (i, (s, enabled, ev)) in [
        (Sym::Previous, has_prev, Event::Previous),
        (
            if playing { Sym::Pause } else { Sym::Play },
            true,
            Event::PlayPause,
        ),
        (Sym::Next, has_next, Event::Next),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::from_center_size(
            pos2(centre + (i as f32 - 1.0) * (size + 14.0), by),
            Vec2::splat(size + if i == 1 { 6.0 } else { 0.0 }),
        );
        if button(ui, painter, r, s, enabled, i == 1, ("player-btn", i)) {
            events.push(ev);
        }
    }
    let close_r = Rect::from_center_size(pos2(c.right() - 12.0, by), Vec2::splat(22.0));
    if button(
        ui,
        painter,
        close_r,
        Sym::Close,
        true,
        false,
        "player-close",
    ) {
        events.push(Event::Close);
    }
    let fs_r = Rect::from_center_size(
        pos2(c.right() - if n > 1 { 72.0 } else { 42.0 }, by),
        Vec2::splat(22.0),
    );
    if button(
        ui,
        painter,
        fs_r,
        if fullscreen {
            Sym::ExitFullscreen
        } else {
            Sym::Fullscreen
        },
        true,
        false,
        "player-fullscreen",
    ) {
        events.push(Event::Fullscreen);
    }
    if n > 1 {
        let list_r = Rect::from_center_size(pos2(c.right() - 42.0, by), Vec2::splat(22.0));
        if button(
            ui,
            painter,
            list_r,
            Sym::Playlist,
            true,
            p.show_list,
            "player-list",
        ) {
            events.push(Event::ToggleList);
        }
    }
}

enum Event {
    PlayPause,
    Seek(u64),
    Previous,
    Next,
    Jump(usize),
    ToggleList,
    Close,
    Fullscreen,
}

fn button(
    ui: &mut Ui,
    painter: &egui::Painter,
    r: Rect,
    s: Sym,
    enabled: bool,
    strong: bool,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let resp = ui.interact(
        r,
        ui.scope_id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let pal = theme::palette(ui.visuals().dark_mode);
    if strong {
        painter.circle_filled(r.center(), r.width() / 2.0, theme::ACCENT);
    } else if resp.hovered() && enabled {
        painter.circle_filled(r.center(), r.width() / 2.0, pal.hover);
    }
    let color = if strong {
        Color32::WHITE
    } else if enabled {
        ui.visuals().text_color()
    } else {
        pal.weak.gamma_multiply(0.5)
    };
    icons::sym(
        painter,
        r.shrink(r.width() * if strong { 0.28 } else { 0.18 }),
        s,
        color,
    );
    resp.clicked() && enabled
}

/// Keys in full screen: Esc leaves it, Space plays or pauses, ← → go back or on 10 s,
/// Page Up / Page Down change file.
pub fn fullscreen_keys(app: &mut App, ctx: &egui::Context) {
    use egui::{Key, Modifiers};
    let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
    let mut events = Vec::new();
    if pressed(Key::Escape) {
        events.push(Event::Fullscreen);
    }
    if pressed(Key::Space) || pressed(Key::K) {
        events.push(Event::PlayPause);
    }
    let pos = app
        .player
        .as_ref()
        .map(|p| p.state.position_ms)
        .unwrap_or(0);
    if pressed(Key::ArrowRight) {
        events.push(Event::Seek(pos + 10_000));
    }
    if pressed(Key::ArrowLeft) {
        events.push(Event::Seek(pos.saturating_sub(10_000)));
    }
    if pressed(Key::PageDown) {
        events.push(Event::Next);
    }
    if pressed(Key::PageUp) {
        events.push(Event::Previous);
    }
    if !events.is_empty()
        && let Some(p) = app.player.as_mut()
    {
        p.last_motion = Instant::now();
    }
    apply(app, events);
}

/// The player over the whole window: the picture as big as it fits, the controls along the
/// bottom while the pointer moves (or while paused). A double click leaves full screen.
pub fn fullscreen(app: &mut App, ctx: &egui::Context) {
    let Some(p) = app.player.as_mut() else { return };
    if !p.fullscreen {
        return;
    }
    let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed());
    if moved {
        p.last_motion = Instant::now();
    }
    let idle = p.last_motion.elapsed();
    let visible = !p.state.playing || p.loading || idle < Duration::from_millis(2500);
    if !visible {
        ctx.set_cursor_icon(egui::CursorIcon::None);
    } else if p.state.playing {
        ctx.request_repaint_after(Duration::from_millis(2600).saturating_sub(idle));
    }
    let screen = ctx.content_rect();
    let mut events = Vec::new();
    egui::Area::new(egui::Id::unique("player-fullscreen"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let Some(p) = app.player.as_mut() else { return };
            let backdrop = ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0, Color32::BLACK);
            // The video is a surface of its own over the window: the bar must not lie on it, so
            // while the bar shows the picture sits above it.
            let bar_h = if visible { 76.0 } else { 0.0 };
            let picture =
                Rect::from_min_max(screen.min, pos2(screen.right(), screen.bottom() - bar_h));
            if p.is_video() {
                let aspect = p
                    .state
                    .video
                    .map(|(w, h)| w as f32 / h.max(1) as f32)
                    .unwrap_or(16.0 / 9.0);
                let (w, h) = if picture.width() / picture.height() > aspect {
                    (picture.height() * aspect, picture.height())
                } else {
                    (picture.width(), picture.width() / aspect)
                };
                let r = Rect::from_center_size(picture.center(), vec2(w, h));
                p.video_rect = Some(r);
                if cfg!(not(target_os = "android")) {
                    painter.rect_stroke(
                        r,
                        0,
                        Stroke::new(1.0, Color32::from_gray(60)),
                        egui::StrokeKind::Inside,
                    );
                    icons::sym(
                        &painter,
                        Rect::from_center_size(
                            r.center(),
                            Vec2::splat(r.height().min(r.width()) * 0.2),
                        ),
                        Sym::Film,
                        Color32::from_gray(90),
                    );
                }
            } else if let Some(tex) = &p.cover {
                let size = tex.size_vec2();
                let inner = picture.shrink(40.0);
                let scale = (inner.width() / size.x).min(inner.height() / size.y);
                egui::Image::from_texture(tex)
                    .corner_radius(8)
                    .paint_at(ui, Rect::from_center_size(inner.center(), size * scale));
            } else {
                let side = picture.height().min(picture.width()) * 0.3;
                icons::sym(
                    &painter,
                    Rect::from_center_size(picture.center(), Vec2::splat(side)),
                    Sym::Music,
                    Color32::from_gray(90),
                );
            }
            if visible {
                let bar =
                    Rect::from_min_max(pos2(screen.left(), screen.bottom() - bar_h), screen.max);
                painter.rect_filled(bar, 0, Color32::from_rgb(0x16, 0x16, 0x18));
                let c = Rect::from_min_max(bar.min + vec2(24.0, 8.0), bar.max - vec2(24.0, 4.0));
                let title = p.current().map(|e| e.name.clone()).unwrap_or_default();
                controls(
                    p,
                    ui,
                    &painter,
                    c,
                    title,
                    Color32::from_gray(235),
                    Color32::from_gray(150),
                    Color32::from_gray(70),
                    true,
                    &mut events,
                );
            }
            if backdrop.double_clicked() {
                events.push(Event::Fullscreen);
            }
        });
    apply(app, events);
}
