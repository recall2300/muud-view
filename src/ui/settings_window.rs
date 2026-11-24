//! F5 settings: general, view, mouse, keys, photo folders, Windows.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Id, Order, Rect, Sense, StrokeKind, UiBuilder,
};

use crate::app::{MuudApp, VERSION};
use crate::core::keymap::{self, Binding, Cmd, Keymap};
use crate::core::photo_folders;
use crate::core::settings::{Background, Lang, MouseAction, Osd, SortBy, WheelMode};
use crate::platform::integration;
use crate::ui::i18n::tr;
use crate::ui::setup::ACTIONS;
use crate::ui::theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    General,
    View,
    Mouse,
    Keys,
    Folders,
    Windows,
}

const TABS: &[(Tab, &str)] = &[
    (Tab::General, "GENERAL"),
    (Tab::View, "VIEW"),
    (Tab::Mouse, "MOUSE"),
    (Tab::Keys, "KEYS"),
    (Tab::Folders, "FOLDERS"),
    (Tab::Windows, "WINDOWS"),
];

const ROW: f32 = 34.0;

/// Human name of a command for the key list.
pub fn cmd_label(cmd: Cmd) -> String {
    use crate::ui::i18n::is_korean;
    match cmd {
        Cmd::CopyToFolder(n) if is_korean() => format!("보관함 {n}에 복사"),
        Cmd::MoveToFolder(n) if is_korean() => format!("보관함 {n}로 이동"),
        Cmd::Slideshow(n) if is_korean() => format!("슬라이드쇼 {n}초"),
        Cmd::CopyToFolder(n) => format!("COPY TO FOLDER {n}"),
        Cmd::MoveToFolder(n) => format!("MOVE TO FOLDER {n}"),
        Cmd::Slideshow(n) => format!("SLIDESHOW {n}S"),
        other => {
            let en = other.id().replace('_', " ").to_uppercase();
            let ko = match other {
                Cmd::Next => "다음",
                Cmd::Prev => "이전",
                Cmd::First => "처음",
                Cmd::Last => "끝",
                Cmd::Forward10 => "10장 앞으로",
                Cmd::Back10 => "10장 뒤로",
                Cmd::RandomNext => "무작위 다음",
                Cmd::RandomPrev => "무작위 이전",
                Cmd::FitWindow => "창 맞춤",
                Cmd::FitWidth => "폭 맞춤",
                Cmd::Original => "원본 100%",
                Cmd::ZoomIn => "확대",
                Cmd::ZoomOut => "축소",
                Cmd::LockZoom => "배율 고정",
                Cmd::StretchSmall => "작은 이미지 늘리기",
                Cmd::Fullscreen => "전체화면",
                Cmd::Maximize => "최대화",
                Cmd::AlwaysOnTop => "항상 위",
                Cmd::SmoothOn => "부드럽게 확대",
                Cmd::SmoothOff => "픽셀 그대로",
                Cmd::CycleBackground => "배경 바꾸기",
                Cmd::CycleOsd => "화면 표시 바꾸기",
                Cmd::ToggleInfo => "정보 패널",
                Cmd::Minimap => "미니맵",
                Cmd::Compare => "두 이미지 비교",
                Cmd::CompareMode => "비교 보기 바꾸기",
                Cmd::PinTop => "상단 바 고정",
                Cmd::PinBottom => "하단 바 고정",
                Cmd::RotateCw => "시계 방향 회전",
                Cmd::RotateCcw => "반시계 방향 회전",
                Cmd::Rotate180 => "180° 회전",
                Cmd::ResetRotation => "회전 원래대로",
                Cmd::FlipH => "좌우 뒤집기",
                Cmd::FlipV => "상하 뒤집기",
                Cmd::CopyImage => "이미지 + 파일 복사",
                Cmd::CopyResized => "작게 복사",
                Cmd::CopyExif => "EXIF 복사",
                Cmd::CopyPath => "경로 복사",
                Cmd::Paste => "클립보드 보기",
                Cmd::Undo => "되돌리기",
                Cmd::Trash => "휴지통으로",
                Cmd::DeletePermanently => "완전 삭제",
                Cmd::Rename => "이름 바꾸기",
                Cmd::ShowInExplorer => "탐색기에서 보기",
                Cmd::OpenFile => "파일 열기",
                Cmd::OpenFolder => "폴더 열기",
                Cmd::Folders => "최근 폴더",
                Cmd::CloseFile => "파일 닫기",
                Cmd::StopSlideshow => "슬라이드쇼 정지",
                Cmd::Help => "단축키 표",
                Cmd::Setup => "설정",
                Cmd::Exit => "종료",
                Cmd::Escape => "닫기 / 전체화면 해제",
                _ => "",
            };
            if is_korean() && !ko.is_empty() {
                ko.to_string()
            } else {
                en
            }
        }
    }
}

impl MuudApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let w = 780.0f32.min(screen.width() - 32.0);
        let h = 620.0f32.min(screen.height() - 32.0);
        let card = Rect::from_center_size(screen.center(), vec2(w, h));
        let before = self.settings.clone();

        egui::Area::new(Id::unique("settings-window"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, backdrop) = ui.allocate_exact_size(screen.size(), Sense::click());
                let p = ui.painter().clone();
                p.rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::BLACK.gamma_multiply(0.6),
                );
                p.rect_filled(card, CornerRadius::ZERO, paper());
                p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);

                let x0 = card.left() + 24.0;
                let r = p.text(
                    pos2(x0, card.top() + 18.0),
                    Align2::LEFT_TOP,
                    tr("SETTINGS"),
                    grotesk(30.0),
                    ink(),
                );
                pill(
                    &p,
                    pos2(r.right() + 14.0, r.center().y),
                    &format!("v{VERSION}"),
                    ink(),
                );
                p.text(
                    pos2(card.right() - 24.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    tr("ESC CLOSE"),
                    mono(11.0),
                    muted(),
                );

                // Tabs.
                let mut x = x0;
                let ty = r.bottom() + 22.0;
                for (tab, label) in TABS {
                    let (w, hit) = toggle_pill(
                        ui,
                        pos2(x, ty),
                        tr(label),
                        self.settings_tab == *tab,
                        ("tab", *label),
                    );
                    if hit {
                        self.settings_tab = *tab;
                        self.capture = None;
                    }
                    x += w + 6.0;
                }
                let line_y = ty + 22.0;
                p.line_segment(
                    [pos2(card.left(), line_y), pos2(card.right(), line_y)],
                    hair(),
                );

                let content = Rect::from_min_max(
                    pos2(card.left() + 1.0, line_y + 1.0),
                    pos2(card.right() - 1.0, card.bottom() - 1.0),
                );
                ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.add_space(10.0);
                            match self.settings_tab {
                                Tab::General => self.tab_general(ui),
                                Tab::View => self.tab_view(ui),
                                Tab::Mouse => self.tab_mouse(ui),
                                Tab::Keys => self.tab_keys(ui),
                                Tab::Folders => self.tab_folders(ui),
                                Tab::Windows => self.tab_windows(ui),
                            }
                            ui.add_space(16.0);
                        });
                });

                let outside = ui
                    .ctx()
                    .input(|i| i.pointer.hover_pos().is_some_and(|p| !card.contains(p)));
                if backdrop.clicked() && outside {
                    self.show_settings = false;
                    self.capture = None;
                }
            });

        self.apply_settings_change(ctx, &before);
    }

    /// Applies side effects of settings edits (language, theme, keys, order).
    pub(crate) fn apply_settings_change(
        &mut self,
        ctx: &egui::Context,
        before: &crate::core::settings::Settings,
    ) {
        let s = self.settings.clone();
        if s.lang != before.lang {
            crate::ui::i18n::set_korean(s.lang == Lang::Ko);
        }
        if s.dark != before.dark {
            set_dark(s.dark);
            apply_style(ctx);
        }
        if s.key_overrides != before.key_overrides {
            self.keymap = Keymap::with_overrides(&self.settings.key_overrides);
        }
        if s.sort != before.sort || s.sort_desc != before.sort_desc {
            if let Some(cur) = self.lib.current().cloned() {
                self.open(&cur);
            }
        }
        if s.smooth != before.smooth {
            self.textures.clear();
        }
        if s.color_manage != before.color_manage {
            self.lib.loader().set_color_management(s.color_manage);
            self.lib.reload();
        }
    }

    fn tab_general(&mut self, ui: &mut egui::Ui) {
        section(ui, "GENERAL");
        if let Some(i) = choice_row(
            ui,
            "LANGUAGE",
            &["한국어", "ENGLISH"],
            (self.settings.lang == Lang::En) as usize,
        ) {
            self.settings.lang = if i == 0 { Lang::Ko } else { Lang::En };
        }
        if let Some(i) = choice_row(
            ui,
            "THEME",
            &[tr("LIGHT"), tr("DARK")],
            self.settings.dark as usize,
        ) {
            self.settings.dark = i == 1;
        }
        if let Some(i) = choice_row(
            ui,
            "RESUME WHERE I LEFT OFF",
            &[tr("ON"), tr("OFF")],
            (!self.settings.resume) as usize,
        ) {
            self.settings.resume = i == 0;
        }
        let sort = match self.settings.sort {
            SortBy::Name => 0,
            SortBy::Modified => 1,
            SortBy::Size => 2,
        };
        if let Some(i) = choice_row(ui, "SORT BY", &[tr("NAME"), tr("DATE"), tr("SIZE")], sort) {
            self.settings.sort = [SortBy::Name, SortBy::Modified, SortBy::Size][i];
        }
        if let Some(i) = choice_row(
            ui,
            "ORDER",
            &[tr("ASCENDING"), tr("DESCENDING")],
            self.settings.sort_desc as usize,
        ) {
            self.settings.sort_desc = i == 1;
        }
    }

    fn tab_view(&mut self, ui: &mut egui::Ui) {
        section(ui, "VIEW");
        let bg = match self.settings.background {
            Background::Ink => 0,
            Background::Paper => 1,
            Background::Gray => 2,
        };
        if let Some(i) = choice_row(ui, "BACKGROUND", &[tr("INK"), tr("PAPER"), tr("GRAY")], bg) {
            self.settings.background = [Background::Ink, Background::Paper, Background::Gray][i];
        }
        if let Some(i) = choice_row(
            ui,
            "SMOOTH SCALING",
            &[tr("ON"), tr("OFF")],
            (!self.settings.smooth) as usize,
        ) {
            self.settings.smooth = i == 0;
        }
        if let Some(i) = choice_row(
            ui,
            "MINIMAP",
            &[tr("ON"), tr("OFF")],
            (!self.settings.minimap) as usize,
        ) {
            self.settings.minimap = i == 0;
        }
        if let Some(i) = choice_row(
            ui,
            "COLOR PROFILE",
            &[tr("ON"), tr("OFF")],
            (!self.settings.color_manage) as usize,
        ) {
            self.settings.color_manage = i == 0;
        }
        if let Some(i) = choice_row(
            ui,
            "STRETCH SMALL IMAGES",
            &[tr("ON"), tr("OFF")],
            (!self.settings.stretch_small) as usize,
        ) {
            self.settings.stretch_small = i == 0;
        }
        if let Some(i) = choice_row(
            ui,
            "TOP BAR",
            &[tr("PINNED"), tr("AUTO-HIDE")],
            (!self.settings.pin_top) as usize,
        ) {
            self.settings.pin_top = i == 0;
        }
        if let Some(i) = choice_row(
            ui,
            "BOTTOM BAR",
            &[tr("PINNED"), tr("AUTO-HIDE")],
            (!self.settings.pin_bottom) as usize,
        ) {
            self.settings.pin_bottom = i == 0;
        }
        let osd = match self.settings.osd {
            Osd::Off => 0,
            Osd::Name => 1,
            Osd::Full => 2,
        };
        if let Some(i) = choice_row(
            ui,
            "ON-SCREEN INFO",
            &[tr("OFF"), tr("NAME"), tr("FULL")],
            osd,
        ) {
            self.settings.osd = [Osd::Off, Osd::Name, Osd::Full][i];
        }
        if let Some(i) = choice_row(
            ui,
            "HIDE CURSOR IN FULLSCREEN",
            &[tr("ON"), tr("OFF")],
            (!self.settings.cursor_autohide) as usize,
        ) {
            self.settings.cursor_autohide = i == 0;
        }
    }

    fn tab_mouse(&mut self, ui: &mut egui::Ui) {
        section(ui, "MOUSE");
        let wheel = (self.settings.wheel == WheelMode::Zoom) as usize;
        if let Some(i) = choice_row(ui, "WHEEL", &[tr("NAVIGATE"), tr("ZOOM")], wheel) {
            self.settings.wheel = if i == 0 {
                WheelMode::Navigate
            } else {
                WheelMode::Zoom
            };
        }
        if let Some(i) = choice_row(
            ui,
            "WHEEL PAST THE EDGE → NEXT",
            &[tr("ON"), tr("OFF")],
            (!self.settings.wheel_edge_next) as usize,
        ) {
            self.settings.wheel_edge_next = i == 0;
        }
        let actions = [
            MouseAction::None,
            MouseAction::Fullscreen,
            MouseAction::Maximize,
            MouseAction::Original,
            MouseAction::FitWindow,
        ];
        let names = [
            tr("NONE"),
            tr("FULLSCREEN"),
            tr("MAXIMIZE"),
            "100%",
            tr("FIT"),
        ];
        let idx = |a: MouseAction| actions.iter().position(|x| *x == a).unwrap_or(0);
        if let Some(i) = choice_row(ui, "MIDDLE CLICK", &names, idx(self.settings.middle_click)) {
            self.settings.middle_click = actions[i];
        }
        if let Some(i) = choice_row(ui, "DOUBLE CLICK", &names, idx(self.settings.double_click)) {
            self.settings.double_click = actions[i];
        }
        if let Some(i) = choice_row(
            ui,
            "SIDE BUTTONS → PREV / NEXT",
            &[tr("ON"), tr("OFF")],
            (!self.settings.side_buttons) as usize,
        ) {
            self.settings.side_buttons = i == 0;
        }
    }

    fn tab_keys(&mut self, ui: &mut egui::Ui) {
        // Toolbar.
        let (bar, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW + 6.0), Sense::hover());
        let mut x = bar.left() + 24.0;
        for (label, which) in [("RESET ALL", 0), ("EXPORT", 1), ("IMPORT", 2)] {
            let (w, hit) = toggle_pill(
                ui,
                pos2(x, bar.center().y),
                tr(label),
                false,
                ("keys-tool", which),
            );
            if hit {
                self.keys_tool(which);
            }
            x += w + 6.0;
        }
        if let Some(cmd) = self.capture {
            let msg = format!("{} — {}", cmd_label(cmd), tr("PRESS A KEY · ESC CANCEL"));
            ui.painter().text(
                pos2(x + 12.0, bar.center().y),
                Align2::LEFT_CENTER,
                msg,
                mono_medium(12.0),
                ink(),
            );
        }

        let overridden: Vec<String> = self
            .settings
            .key_overrides
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        for cmd in keymap::ALL {
            let (row, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW - 4.0), Sense::hover());
            let p = ui.painter().clone();
            let cy = row.center().y;
            let lx = row.left() + 24.0;
            let label = cmd_label(*cmd);
            p.text(pos2(lx, cy), Align2::LEFT_CENTER, label, mono(12.5), ink());
            let mut kx = row.left() + 260.0;
            let keys = self.keymap.keys_for(*cmd);
            for b in &keys {
                let (w, hit) = toggle_pill(
                    ui,
                    pos2(kx, cy),
                    &b.display(),
                    false,
                    ("key", cmd.id(), b.label()),
                );
                if hit {
                    // Clicking a key removes it from this command.
                    let rest: Vec<String> = keys
                        .iter()
                        .filter(|x| *x != b)
                        .map(Binding::label)
                        .collect();
                    self.settings
                        .key_overrides
                        .retain(|(id, _)| *id != cmd.id());
                    self.settings.key_overrides.push((cmd.id(), rest));
                }
                kx += w + 4.0;
            }
            let capturing = self.capture == Some(*cmd);
            let (w, hit) = toggle_pill(
                ui,
                pos2(kx + 4.0, cy),
                &format!("+ {}", tr("ADD")),
                capturing,
                ("add", cmd.id()),
            );
            if hit {
                self.capture = if capturing { None } else { Some(*cmd) };
            }
            if overridden.contains(&cmd.id()) {
                let (_, hit) = toggle_pill(
                    ui,
                    pos2(kx + 12.0 + w, cy),
                    tr("RESET"),
                    false,
                    ("reset", cmd.id()),
                );
                if hit {
                    self.settings.key_overrides = keymap::reset(&self.settings.key_overrides, *cmd);
                }
            }
            dotted_rule(
                &p,
                pos2(lx, row.bottom()),
                pos2(row.right() - 24.0, row.bottom()),
                ink().gamma_multiply(0.35),
            );
        }
    }

    fn keys_tool(&mut self, which: u8) {
        let path = crate::core::log::data_dir().join("keymap.txt");
        match which {
            0 => {
                self.settings.key_overrides.clear();
                self.notify("KEYS RESET");
            }
            1 => {
                let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
                match std::fs::write(&path, keymap::export(&self.keymap)) {
                    Ok(()) => {
                        let _ = crate::platform::show_in_explorer(&path);
                        self.notify(format!("KEYMAP EXPORTED · {}", path.display()));
                    }
                    Err(e) => self.notify(format!("FAILED: {e}")),
                }
            }
            _ => match std::fs::read_to_string(&path) {
                Ok(text) => {
                    self.settings.key_overrides = keymap::import(&text);
                    self.notify("KEYMAP IMPORTED");
                }
                Err(e) => self.notify(format!("FAILED: {} ({e})", path.display())),
            },
        }
    }

    /// Called from key handling while `capture` is set.
    pub(crate) fn capture_key(&mut self, key: egui::Key, mods: egui::Modifiers) {
        let Some(cmd) = self.capture.take() else {
            return;
        };
        if key == egui::Key::Escape {
            return;
        }
        let b = Binding::from_event(key, mods);
        let (ov, lost) = keymap::assign(&self.settings.key_overrides, cmd, b, false);
        self.settings.key_overrides = ov;
        match lost {
            Some(other) => self.notify(format!(
                "{} → {} ({} ✕)",
                b.display(),
                cmd_label(cmd),
                cmd_label(other)
            )),
            None => self.notify(format!("{} → {}", b.display(), cmd_label(cmd))),
        }
    }

    fn tab_folders(&mut self, ui: &mut egui::Ui) {
        section(ui, "PHOTO FOLDERS");
        for slot in 1..=photo_folders::SLOTS as u8 {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
            let p = ui.painter().clone();
            let cy = row.center().y;
            let lx = row.left() + 24.0;
            p.text(
                pos2(lx, cy),
                Align2::LEFT_CENTER,
                format!("{slot}"),
                mono_medium(12.5),
                ink(),
            );
            let path = photo_folders::resolve(&self.settings.photo_folders, slot);
            let custom = self
                .settings
                .photo_folders
                .get(slot as usize - 1)
                .is_some_and(|p| p.is_some());
            let text = ellipsize(
                &p,
                &path.display().to_string(),
                mono(12.0),
                row.width() - 300.0,
            );
            p.text(
                pos2(lx + 30.0, cy),
                Align2::LEFT_CENTER,
                text,
                mono(12.0),
                if custom { ink() } else { muted() },
            );
            let bx = row.right() - 200.0;
            let (w, hit) = toggle_pill(
                ui,
                pos2(bx, cy),
                tr("CHANGE"),
                false,
                ("folder-change", slot),
            );
            if hit {
                if let Some(dir) = crate::platform::pick_folder(Some(&path)) {
                    self.settings
                        .photo_folders
                        .resize(photo_folders::SLOTS, None);
                    self.settings.photo_folders[slot as usize - 1] = Some(dir);
                }
            }
            if custom {
                let (_, hit) = toggle_pill(
                    ui,
                    pos2(bx + w + 6.0, cy),
                    tr("DEFAULT"),
                    false,
                    ("folder-reset", slot),
                );
                if hit {
                    self.settings.photo_folders[slot as usize - 1] = None;
                }
            }
        }
    }

    fn tab_windows(&mut self, ui: &mut egui::Ui) {
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        let mut x = head.left() + 24.0;
        let p = ui.painter().clone();
        for t in [
            if integration::is_registered() {
                tr("REGISTERED")
            } else {
                tr("NOT REGISTERED")
            },
            if integration::portable_data_dir().is_some() {
                tr("PORTABLE")
            } else {
                tr("INSTALLED")
            },
        ] {
            x += pill(&p, pos2(x, head.center().y), t, ink()) + 6.0;
        }
        let mut chosen = None;
        for (i, (action, key, label)) in ACTIONS.iter().enumerate() {
            if i == 0 {
                section(ui, "WINDOWS");
            }
            if i == 3 {
                section(ui, "SUPPORT");
            }
            let (row, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW - 4.0), Sense::hover());
            let resp = ui.interact(
                row.shrink2(vec2(12.0, 0.0)),
                Id::unique(("setup-row", *key)),
                Sense::click(),
            );
            let p = ui.painter();
            let (bg, fg) = if resp.hovered() {
                (ink(), paper())
            } else {
                (Color32::TRANSPARENT, ink())
            };
            p.rect_filled(row.shrink2(vec2(12.0, 0.0)), CornerRadius::ZERO, bg);
            p.text(
                pos2(row.left() + 24.0, row.center().y),
                Align2::LEFT_CENTER,
                *key,
                mono_medium(12.5),
                fg,
            );
            p.text(
                pos2(row.left() + 58.0, row.center().y),
                Align2::LEFT_CENTER,
                tr(label),
                mono(12.5),
                fg,
            );
            if resp.clicked() {
                chosen = Some(*action);
            }
        }
        let ctx = ui.ctx().clone();
        if let Some(a) = chosen {
            self.run_setup(&ctx, a);
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    sub_label(
        ui.painter(),
        pos2(r.left() + 24.0, r.top() + 8.0),
        tr(title),
    );
}

/// Label on the left, choices as pills on the right. Returns a clicked index.
fn choice_row(ui: &mut egui::Ui, label: &str, options: &[&str], selected: usize) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let p = ui.painter().clone();
    let cy = row.center().y;
    p.text(
        pos2(row.left() + 24.0, cy),
        Align2::LEFT_CENTER,
        tr(label),
        mono(12.5),
        ink(),
    );
    let widths: Vec<f32> = options.iter().map(|o| pill_width(&p, o)).collect();
    let mut x =
        row.right() - 24.0 - widths.iter().sum::<f32>() - 6.0 * (options.len() as f32 - 1.0);
    let mut out = None;
    for (i, o) in options.iter().enumerate() {
        let (w, hit) = toggle_pill(ui, pos2(x, cy), o, i == selected, ("choice", label, i));
        if hit {
            out = Some(i);
        }
        x += w + 6.0;
    }
    dotted_rule(
        &p,
        pos2(row.left() + 24.0, row.bottom()),
        pos2(row.right() - 24.0, row.bottom()),
        ink().gamma_multiply(0.35),
    );
    out
}

/// A pill that is filled when active or hovered. Returns (width, clicked).
pub fn toggle_pill(
    ui: &mut egui::Ui,
    left_center: egui::Pos2,
    label: &str,
    active: bool,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> (f32, bool) {
    let p = ui.painter();
    let w = pill_width(p, label);
    let rect = Rect::from_min_size(pos2(left_center.x, left_center.y - 12.0), vec2(w, 24.0));
    let resp = ui.interact(rect, Id::unique(id), Sense::click());
    let filled = active || resp.hovered();
    let (fill, fg) = if filled {
        (ink(), paper())
    } else {
        (Color32::TRANSPARENT, ink())
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), fill);
    p.rect_stroke(
        rect,
        CornerRadius::same(255),
        egui::Stroke::new(1.0, ink()),
        StrokeKind::Inside,
    );
    p.text(rect.center(), Align2::CENTER_CENTER, label, mono(11.5), fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (w, resp.clicked())
}
