//! Dock composition, layout and frame rendering on the existing UI thread.

use std::time::Instant;

use anyhow::{Context, Result};
use nosih_core::{
    bounce,
    frame::{DockEdge, Frame as DockFrame},
    layout::{ItemKind, Metrics, layout},
    magnify::{Magnifier, magnify},
};
use nosih_shell::{
    appbar::Edge,
    power, recycle_bin,
    system::{animations_enabled, primary_monitor},
};
use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::SetTimer};

use crate::scene::{Drag, Frame, Scene, SceneInput, SlotContent, Theme};

use super::{
    App, BADGE_CORNER, BOUNCE_HEIGHT, ENTER_HALO, Entry, FLOAT_MARGIN, LABEL_ROOM, LEAVE_HALO,
    RANGE_IN_ICONS, REORDER_DURATION, REVEAL_STRIP, ReorderMotion, Role, SHADOW_ROOM,
    SIDE_LABEL_MAX, START_ID, Stage, StageKey, TRIM_DELAY_MS, TRIM_TIMER, place, window_id,
    window_of,
};

impl App {
    /// List of items: apps, recents, Downloads, and recycle bin, separated into
    /// sections as in macOS.
    pub(super) fn current_entries(&self) -> Vec<Entry> {
        let separator = || Entry {
            kind: ItemKind::Separator,
            id: None,
            role: Role::Folder,
            running: false,
            members: Vec::new(),
        };
        let icon = |id: &str, role: Role, running: bool| Entry {
            kind: ItemKind::Icon,
            id: Some(id.to_owned()),
            role,
            running,
            members: Vec::new(),
        };

        let mut entries: Vec<Entry> = self
            .roster
            .apps()
            .iter()
            .map(|app| match self.roster.group(&app.id) {
                Some(group) => Entry {
                    members: group.apps.clone(),
                    ..icon(&app.id, Role::Group, app.running)
                },
                None => icon(&app.id, Role::App, app.running),
            })
            .collect();
        if self.config.show_start {
            entries.insert(0, icon(START_ID, Role::Start, false));
        }
        if self.config.show_recents && !self.roster.recent().is_empty() {
            entries.push(separator());
            entries.extend(
                self.roster
                    .recent()
                    .iter()
                    .map(|id| icon(id, Role::App, false)),
            );
        }
        entries.push(separator());
        // Unless "show minimized windows" is on, minimized windows hide in
        // their app's icon instead of getting a thumbnail here.
        if self.config.show_minimized {
            entries.extend(
                self.minimized
                    .iter()
                    .map(|&hwnd| icon(&window_id(hwnd), Role::Window, false)),
            );
        }
        if let Some(folder) = &self.config.shortcuts_folder {
            entries.push(icon(folder, Role::Shortcuts, false));
        }
        if self.config.show_downloads
            && let Some(downloads) = &self.downloads
        {
            entries.push(icon(downloads, Role::Folder, false));
        }
        if self.config.show_trash {
            entries.push(icon(recycle_bin::PARSING_NAME, Role::Trash, false));
        }
        // Hiding fixed items can leave a section empty. Never leave loose dividers.
        let mut previous_separator = true;
        entries.retain(|entry| {
            let separator = entry.kind == ItemKind::Separator;
            let keep = !separator || !previous_separator;
            previous_separator = separator;
            keep
        });
        while entries
            .last()
            .is_some_and(|entry| entry.kind == ItemKind::Separator)
        {
            entries.pop();
        }
        entries
    }

    /// A wallpaper change does not change scene keys. Refresh the host samples
    /// in place, preserving magnification, drag state and open previews/panels.
    pub(super) fn refresh_backdrops(&mut self) -> Result<()> {
        if let Some(stage) = &self.stage {
            self.gfx.refresh_backdrops(&stage.scene.root)?;
        }
        if let Some(bar) = &self.menubar {
            bar.refresh_backdrops(&self.gfx)?;
        }
        if let Some(preview) = &self.preview {
            preview.refresh_backdrops(&self.gfx)?;
        }
        if let Some(group) = &self.group_view {
            group.refresh_backdrops(&self.gfx)?;
        }
        Ok(())
    }

    /// Recalculates sizes, positions, and scene for the current state. Does
    /// nothing if the scene inputs are unchanged, since many Windows notifications
    /// do not affect the dock.
    pub(super) fn refresh(&mut self) -> Result<()> {
        if let Some(bar) = &mut self.menubar
            && let Err(error) = bar.refresh(&self.gfx)
        {
            crate::recovery::graphics_error(&error);
            nosih_shell::log::line(&format!("actualización de la barra superior: {error:#}"));
        }
        let monitor = self
            .dock_monitor()
            .context("no se pudo consultar el monitor del dock")?;
        let edge = self.position();
        let key = StageKey {
            screen: monitor.screen,
            edge,
            top_inset: self.menu_bar_inset(monitor.screen, edge),
            scale: monitor.scale,
            look: crate::appearance::current(),
            bin_full: self.bin_full,
            auto_hide: self.config.auto_hide,
            smart_hide: self.config.smart_hide,
            icon_size: self.config.icon_size,
            density: self.config.dock_density,
            max_scale: self.config.max_scale(),
            indicators: self.config.show_indicators,
            entries: self.current_entries(),
        };
        if self.stage.as_ref().is_some_and(|stage| stage.key == key) {
            return Ok(());
        }
        self.close_preview();
        // An unfolded group shows its apps as they were: it closes if they changed.
        if let Some(view) = &self.group_view
            && self
                .roster
                .group(&view.group_id)
                .is_none_or(|group| group.apps.as_slice() != view.apps())
        {
            self.close_group_view();
        }

        let scale = monitor.scale;
        let max_scale = key.max_scale;
        let (gap, padding) = key.density.spacing();
        let mut metrics = Metrics {
            icon: key.icon_size as f32,
            gap,
            padding,
            ..Metrics::DEFAULT
        }
        .scaled(scale);
        let theme = Theme::new(key.look);
        let px = |logical: f32| (logical * scale).round();
        let screen = monitor.screen;
        let screen_width = (screen.right - screen.left) as f32;
        // On a side, the dock runs down the height left under the menu bar.
        let span_start = (screen.top + key.top_inset) as f32;
        let length_room = if edge.is_vertical() {
            screen.bottom as f32 - span_start
        } else {
            screen_width
        };
        let kinds: Vec<ItemKind> = key.entries.iter().map(|entry| entry.kind).collect();
        let mut dock = layout(&kinds, &metrics);
        let available = (length_room - 2.0 * px(FLOAT_MARGIN)).max(1.0);
        // A side dock often fills the height. Magnified, it spreads by up to
        // half its growth past each end, which would put the first icon
        // under the menu bar and the last off screen: room is kept for it.
        // It scales with the icons, so one ratio fits both exactly.
        let spread = if edge.is_vertical() {
            Magnifier {
                range: RANGE_IN_ICONS * (metrics.icon + metrics.gap),
                max_scale,
            }
            .max_growth()
        } else {
            0.0
        };
        if dock.width + spread > available {
            // Keep the roster reachable on a smaller display without
            // changing the user's saved icon size.
            metrics = metrics.scaled(available / (dock.width + spread));
            dock = layout(&kinds, &metrics);
        }
        dock.width = dock.width.round();
        dock.height = dock.height.round();

        self.entries = key.entries.clone();
        self.prepare_assets(
            (metrics.icon * max_scale).round().max(1.0) as u32,
            scale,
            theme.light,
            edge,
        );

        let magnifier = Magnifier {
            range: RANGE_IN_ICONS * (metrics.icon + metrics.gap),
            max_scale,
        };

        let side_growth = (magnifier.max_growth() / 2.0).ceil();
        let rise = (metrics.icon * (max_scale - 1.0)).ceil();

        // Position relative to the screen, not the work area; the work area
        // already excludes the strip reserved by the dock.
        let margin = px(FLOAT_MARGIN);
        let centred = span_start + ((length_room - dock.width) / 2.0).round();
        let (dock_x, dock_y) = match edge {
            DockEdge::Bottom => (
                screen.left as f32 + ((screen_width - dock.width) / 2.0).round(),
                screen.bottom as f32 - margin - dock.height,
            ),
            DockEdge::Left => (screen.left as f32 + margin, centred),
            DockEdge::Right => (screen.right as f32 - margin - dock.height, centred),
        };
        let frame = DockFrame::new(edge, (dock_x, dock_y), dock.height);
        // A hidden dock leaves the whole screen to maximized windows.
        let strip = if self.config.hides_automatically() {
            0
        } else {
            (dock.height + 2.0 * margin) as i32
        };
        // The resting dock's box on screen: across the edge it is as deep as
        // the layout is tall.
        let (box_width, box_height) = if edge.is_vertical() {
            (dock.height, dock.width)
        } else {
            (dock.width, dock.height)
        };
        let (dock_right, dock_bottom) = (dock_x + box_width, dock_y + box_height);
        // Room around the dock, in its own terms: before and after it along
        // the edge, inward (where icons grow and labels show) and outward.
        // Outward never passes the screen edge: a display beyond it must not
        // receive the sliding dock or its mouse input while it slides.
        let zone = move |before: f32, inward: f32, after: f32, outward: f32| match edge {
            DockEdge::Bottom => RECT {
                left: (dock_x - before) as i32,
                top: (dock_y - inward) as i32,
                right: (dock_right + after) as i32,
                bottom: ((dock_bottom + outward) as i32).min(screen.bottom),
            },
            DockEdge::Left => RECT {
                left: ((dock_x - outward) as i32).max(screen.left),
                top: (dock_y - before) as i32,
                right: (dock_right + inward) as i32,
                bottom: (dock_bottom + after) as i32,
            },
            DockEdge::Right => RECT {
                left: (dock_x - inward) as i32,
                top: (dock_y - before) as i32,
                right: ((dock_right + outward) as i32).min(screen.right),
                bottom: (dock_bottom + after) as i32,
            },
        };

        let room = px(SHADOW_ROOM);
        // Labels show inward of the icon: above it on the bottom, beside it
        // on a side, where they need their full width.
        let label_room = if edge.is_vertical() {
            let widest = self
                .assets
                .values()
                .filter_map(|asset| asset.label.as_ref())
                .chain(&self.remove_label)
                .map(|label| label.width)
                .fold(0.0_f32, f32::max);
            px(12.0) + widest.min(px(SIDE_LABEL_MAX))
        } else {
            px(LABEL_ROOM)
        };
        let (room_x, room_top) = (room + side_growth, rise + label_room);
        let window_zone = zone(room_x, room_top, room_x, room);
        let window_frame = DockFrame::new(
            edge,
            (
                dock_x - window_zone.left as f32,
                dock_y - window_zone.top as f32,
            ),
            dock.height,
        );
        let contents: Vec<SlotContent<'_>> = self
            .entries
            .iter()
            .map(|entry| {
                let asset = entry.id.as_ref().and_then(|id| self.assets.get(id));
                let badge = entry
                    .id
                    .as_deref()
                    .and_then(window_of)
                    .and_then(|hwnd| self.windows.iter().find(|window| window.hwnd == hwnd))
                    .and_then(|window| self.assets.get(&window.app_id))
                    .and_then(|asset| asset.icon.as_ref());
                SlotContent {
                    icon: asset.and_then(|asset| asset.icon.as_ref()),
                    label: asset.and_then(|asset| asset.label.as_ref()),
                    running: entry.running && self.config.show_indicators,
                    badge,
                }
            })
            .collect();
        let scene = Scene::build(
            &self.gfx,
            &SceneInput {
                layout: &dock,
                contents: &contents,
                theme,
                scale,
                frame: window_frame,
                remove_label: self.remove_label.as_ref(),
            },
        )?;
        let reorder = self.stage.as_ref().and_then(|previous| {
            // Do not interpolate between monitors or on a theme-only rebuild.
            if self.fullscreen
                || previous.key.screen != key.screen
                || previous.key.edge != key.edge
                || previous.scale != scale
                || previous
                    .key
                    .entries
                    .iter()
                    .map(|entry| &entry.id)
                    .eq(key.entries.iter().map(|entry| &entry.id))
                || !animations_enabled()
            {
                return None;
            }
            let state = magnify(&dock, self.cursor, self.amount.value, &magnifier);
            let offsets: Vec<f32> = key
                .entries
                .iter()
                .zip(&state.rects)
                .map(|(entry, rect)| {
                    let Some(id) = entry.id.as_deref() else {
                        return 0.0;
                    };
                    if self
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.id == id && !drag.from_group)
                    {
                        return 0.0;
                    }
                    let Some(index) = previous
                        .key
                        .entries
                        .iter()
                        .position(|entry| entry.id.as_deref() == Some(id))
                    else {
                        return 0.0;
                    };
                    // Start from the currently drawn icon, including any earlier
                    // move still in progress when the user crosses another slot.
                    let along = if edge.is_vertical() { dock_y } else { dock_x };
                    previous.scene.slot_along(index).map_or(0.0, |drawn| {
                        previous.along_origin() + drawn - along - rect.x
                    })
                })
                .collect();
            offsets
                .iter()
                .any(|offset| offset.abs() >= 0.5)
                .then(|| ReorderMotion {
                    started: Instant::now(),
                    offsets,
                })
        });
        self.target.SetRoot(&scene.root)?;

        let (enter, leave) = (px(ENTER_HALO), px(LEAVE_HALO));
        let stage = Stage {
            key,
            window_zone,
            scene,
            reorder,
            magnifier,
            icon: metrics.icon,
            gap: metrics.gap,
            scale,
            frame,
            window_frame,
            rest_zone: zone(enter, enter, enter, enter),
            hover_zone: zone(
                side_growth + leave,
                rise + leave,
                side_growth + leave,
                leave,
            ),
            // The thin strip at the screen edge that reveals a hidden dock,
            // as long as the dock (plus how far its icons spread).
            edge_zone: match edge {
                DockEdge::Bottom => RECT {
                    left: (dock_x - side_growth) as i32,
                    top: screen.bottom - REVEAL_STRIP,
                    right: (dock_right + side_growth) as i32,
                    bottom: screen.bottom,
                },
                DockEdge::Left => RECT {
                    left: screen.left,
                    top: (dock_y - side_growth) as i32,
                    right: screen.left + REVEAL_STRIP,
                    bottom: (dock_bottom + side_growth) as i32,
                },
                DockEdge::Right => RECT {
                    left: screen.right - REVEAL_STRIP,
                    top: (dock_y - side_growth) as i32,
                    right: screen.right,
                    bottom: (dock_bottom + side_growth) as i32,
                },
            },
            slide_distance: dock.height + px(FLOAT_MARGIN) + room,
            layout: dock,
        };
        self.stage = Some(stage);
        self.update_smart_hiding()?;
        self.sync_badge_visuals()?;
        self.place_windows()?;
        self.schedule_trim();
        self.render()?;
        // Last: reserving space is a synchronous call to Explorer (40-60 ms
        // measured), and the dock is already on screen meanwhile.
        let automatic = self.config.hides_automatically();
        if let Some(appbar) = &mut self.appbar {
            // Auto-hide reserves zero height. Moving the dock must never
            // change the destination monitor's work area.
            let reservation = if automatic {
                primary_monitor().map_or(screen, |primary| primary.screen)
            } else {
                screen
            };
            let side = match edge {
                DockEdge::Bottom => Edge::Bottom,
                DockEdge::Left => Edge::Left,
                DockEdge::Right => Edge::Right,
            };
            appbar.reserve(side, reservation, strip);
        }
        Ok(())
    }

    pub(super) fn place_windows(&self) -> Result<()> {
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        let visible = !self.fullscreen;
        place(self.visual, &stage.window_zone, visible)?;
        // Preserve the expanded input area if the cursor is already over the
        // dock (for example, after launching an app with a dock click).
        let input_zone = if self.hovering || self.drag.is_some() {
            &stage.hover_zone
        } else if self.config.hides_automatically() && self.slide.target == 1.0 {
            &stage.edge_zone
        } else {
            &stage.rest_zone
        };
        place(self.input, input_zone, visible)
    }

    pub(super) fn schedule_trim(&self) {
        // SAFETY: `self.input` is this live window, and this call runs on its UI thread.
        unsafe { SetTimer(Some(self.input), TRIM_TIMER, TRIM_DELAY_MS, None) };
    }

    pub(super) fn trim_if_idle(&self) {
        if !self.hovering && !self.is_animating() {
            power::trim_memory();
        }
    }

    pub(super) fn render(&mut self) -> Result<()> {
        let Some(stage) = &mut self.stage else {
            return Ok(());
        };
        let mut state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value,
            &stage.magnifier,
        );
        let now = Instant::now();
        if let Some(motion) = &stage.reorder {
            let elapsed = now.saturating_duration_since(motion.started);
            if elapsed >= REORDER_DURATION {
                stage.reorder = None;
            } else {
                let remaining = 1.0 - elapsed.as_secs_f32() / REORDER_DURATION.as_secs_f32();
                let ease = remaining * remaining;
                for (rect, offset) in state.rects.iter_mut().zip(&motion.offsets) {
                    rect.x += offset * ease;
                }
            }
        }
        self.hovered = (self.hovering || self.pointed)
            .then(|| state.icon_at(&stage.layout, self.cursor, stage.gap))
            .flatten();

        let lift_of = |id: &str| {
            self.bounces.get(id).map_or(0.0, |bounce| {
                let elapsed = now.duration_since(bounce.start).as_secs_f32();
                bounce::lift(elapsed, stage.icon * BOUNCE_HEIGHT)
            })
        };
        // A group bounces for any of its apps (opening, asking for attention).
        let lifts: Vec<f32> = self
            .entries
            .iter()
            .map(|entry| match entry.role {
                Role::Group => entry
                    .members
                    .iter()
                    .map(|app| lift_of(app.as_str()))
                    .fold(0.0, f32::max),
                _ => entry.id.as_deref().map_or(0.0, lift_of),
            })
            .collect();

        let drag = self.drag.as_ref().and_then(|drag| {
            Some(Drag {
                index: self
                    .entries
                    .iter()
                    .position(|entry| entry.id.as_deref() == Some(drag.id.as_str()))?,
                center: stage.to_dock(drag.point),
                removing: drag.removing,
                ghost: drag.from_group,
            })
        });
        let target = self
            .drag
            .as_ref()
            .and_then(|drag| drag.merge.as_deref())
            .and_then(|merge| {
                self.entries
                    .iter()
                    .position(|entry| entry.id.as_deref() == Some(merge))
            });
        if drag.is_some() {
            self.hovered = None;
        }
        // The open group's name would cover its unfolded apps.
        let open_group = self.group_view.as_ref().and_then(|view| {
            self.entries
                .iter()
                .position(|entry| entry.id.as_deref() == Some(view.group_id.as_str()))
        });
        let at_rest = self.amount.value == 0.0
            && self.bounces.is_empty()
            && self.slide.is_settled()
            && stage.reorder.is_none()
            && drag.is_none();
        let slide = self.slide.value * stage.slide_distance;
        stage.scene.apply(&Frame {
            state: &state,
            hovered: self.hovered.filter(|&index| {
                self.preview.is_none() && !self.menu_open && open_group != Some(index)
            }),
            lifts: &lifts,
            drag,
            target,
            slide,
            snap: at_rest,
        })?;

        let visible = !self.fullscreen;
        for (entry, rect) in self.entries.iter().zip(&state.rects) {
            if let Some(hwnd) = entry.id.as_deref().and_then(window_of) {
                let box_ = stage.window_frame.rect(*rect);
                let (slide_x, slide_y) = stage.window_frame.outward(slide);
                let slot = (box_.x + slide_x, box_.y + slide_y, box_.width, box_.height);
                let has_badge = self
                    .windows
                    .iter()
                    .find(|window| window.hwnd == hwnd)
                    .and_then(|window| self.assets.get(&window.app_id))
                    .is_some_and(|asset| asset.icon.is_some());
                let corner = if has_badge { BADGE_CORNER } else { 0.0 };
                let opacity = match self.flights.iter().find(|flight| flight.hwnd == hwnd) {
                    _ if !visible => 0,
                    Some(flight) => flight.landing_opacity(now),
                    None => 255,
                };
                self.thumbnails.place(hwnd, slot, corner, opacity);
            }
        }
        Ok(())
    }
}
