//! Context menus and deferred shell actions, always outside the App borrow.

use std::{env, sync::atomic::Ordering, thread};

use anyhow::Result;
use nosih_core::{layout::Rect, magnify::magnify};
use nosih_shell::{end_task::PreparedEndTask, jumplist, launcher, pins, tracker};
use windows::Win32::Foundation::{HWND, POINT};

use crate::{
    folder_menu,
    menu::{self, MenuItem},
    shell_work::{self, Action as ShellAction},
    strings::{self, text},
};

use super::{
    App, CMD_CLOSE_WINDOW, CMD_EMPTY_BIN, CMD_END_TASK, CMD_FORGET_RECENT, CMD_HIDE_FIXED,
    CMD_JUMP, CMD_OPEN, CMD_QUIT_APP, CMD_QUIT_DOCK, CMD_REMOVE_FOLDER, CMD_REVEAL,
    CMD_RUN_AS_ADMIN, CMD_SETTINGS, CMD_SHOW_DESKTOP, CMD_TASK_MANAGER, CMD_TASKBAR_SETTINGS,
    CMD_TOGGLE_HIDING, CMD_TOGGLE_PIN, CMD_UNGROUP, CMD_WINDOW, MAIN_WINDOW, MENU_TITLE_CHARS,
    MENU_WINDOWS, MenuRequest, MenuTarget, Outside, Role, Stage,
    events::menu_bar_refresh,
    lifecycle::{main_window, request_quit},
    preferences::{
        explorer_id, fixed_item_preference, forget_shortcuts_folder, hide_fixed_item, save_settings,
    },
    shorten, window_of, with_app, with_app_then,
};

pub(super) fn run_outside(outside: Outside) {
    let submit = |action| {
        let result = shell_work::dispatch(action);
        if let Err(error) = result {
            eprintln!("nosih: operación no iniciada: {error:#}");
            shell_work::notify_not_started();
        }
    };
    match outside {
        Outside::Open(id) => submit(ShellAction::Open { id }),
        Outside::OpenStart => launcher::open_start_menu(),
        Outside::Activate(windows) => launcher::activate(&windows),
        Outside::Minimize(windows) => launcher::minimize(&windows),
        Outside::Reveal(path) => submit(ShellAction::Reveal { path }),
        Outside::OpenWith(app, files) => submit(ShellAction::OpenWith { app, files }),
        Outside::Recycle(files) => submit(ShellAction::Recycle { files }),
        Outside::Transfer {
            folder,
            files: paths,
            copy,
        } => submit(ShellAction::Transfer {
            files: paths,
            folder,
            copy,
        }),
        Outside::EmptyBin => submit(ShellAction::EmptyBin),
        Outside::QuitDock => request_quit(),
        Outside::OpenAsAdmin(id) => submit(ShellAction::OpenAsAdmin { id }),
        Outside::ShowDesktop => launcher::show_desktop(),
        Outside::UnpinFromTaskbar(id) => submit(ShellAction::Unpin { id }),
        Outside::OpenJump { target, program } => submit(ShellAction::OpenJump { target, program }),
        Outside::OpenSettings => {
            // The settings app is installed next to the dock.
            let settings = env::current_exe()
                .ok()
                .map(|exe| exe.with_file_name("nosih-settings.exe"));
            match settings {
                Some(path) if path.exists() => submit(ShellAction::Open {
                    id: path.to_string_lossy().into_owned(),
                }),
                _ => eprintln!("nosih: no se encontró nosih-settings.exe junto al dock"),
            }
        }
        Outside::InputMenu { gfx, anchor, dark } => {
            use nosih_shell::keyboard;
            let context = keyboard::Context::current();
            let layouts = keyboard::layouts(strings::text().keyboard);
            const SETTINGS: u32 = u32::MAX;
            let mut items: Vec<MenuItem> = layouts
                .iter()
                .enumerate()
                .map(|(index, layout)| MenuItem::Action {
                    command: index as u32 + 1,
                    label: layout.name.replace('&', "&&"),
                    checked: context.is_some_and(|context| context.layout == layout.handle),
                    enabled: context.is_some(),
                    icon: None,
                })
                .collect();
            if !items.is_empty() {
                items.push(MenuItem::Separator);
            }
            items.push(MenuItem::action(
                SETTINGS,
                strings::text().keyboard_settings,
            ));
            let owner = main_window();
            with_app("abrir selector de teclado", |app| {
                app.menu_open = true;
                Ok(())
            });
            let chosen = menu::show_below(&gfx, owner, &items, anchor, dark);
            with_app("cerrar selector de teclado", App::after_menu);
            if chosen == Some(SETTINGS) {
                submit(ShellAction::Open {
                    id: "ms-settings:regionlanguage".into(),
                });
            } else if let (Some(context), Some(layout)) = (
                context,
                chosen
                    .and_then(|command| command.checked_sub(1))
                    .and_then(|index| layouts.get(index as usize)),
            ) && let Err(error) = context.request(layout.handle)
            {
                nosih_shell::log::line(&format!("cambiar teclado: {error}"));
                shell_work::notify_not_started();
            }
            menu_bar_refresh();
        }
        Outside::EndTask(target) => {
            // Captured kernel handles are Send; no HWND or PID is resolved
            // again after the menu. Never wait on an unresponsive UI thread.
            let started = thread::Builder::new()
                .name("nosih-end-task".into())
                .spawn(move || {
                    if let Err(error) = target.terminate() {
                        nosih_shell::log::line(&format!("finalizar tarea: {error}"));
                        shell_work::notify_end_task_failed();
                    }
                });
            if started.is_err() {
                shell_work::notify_end_task_failed();
            }
        }
        Outside::Menu(request) => {
            let owner = HWND(MAIN_WINDOW.load(Ordering::Relaxed) as *mut _);
            with_app("abrir menú", |app| {
                app.menu_open = true;
                app.render()
            });
            let chosen = menu::show(
                &request.gfx,
                owner,
                &request.items,
                request.anchor,
                request.dark,
                request.placement,
            );
            with_app("menú cerrado", App::after_menu);
            if let Some(command) = chosen {
                with_app_then("orden del menú", |app| {
                    app.on_command(request.target, command)
                });
            }
        }
        Outside::FolderMenu {
            folder,
            anchor,
            placement,
            dark,
        } => {
            with_app("abrir carpeta de accesos directos", |app| {
                app.menu_open = true;
                app.render()
            });
            let chosen = folder_menu::show(main_window(), &folder, anchor, dark, placement);
            with_app("menú cerrado", App::after_menu);
            if let Some(path) = chosen {
                submit(ShellAction::Open { id: path });
            }
        }
    }
}

impl App {
    /// The shortcuts folder's menu, above its icon. The label hides while
    /// the menu is open, as for the context menu.
    pub(super) fn folder_menu(&mut self, folder: String) -> Result<Option<Outside>> {
        let Some(anchor) = self.hovered.and_then(|index| self.anchor_beside(index)) else {
            return Ok(None);
        };
        let placement = self
            .stage
            .as_ref()
            .map_or(menu::Placement::Above, Stage::menu_placement);
        self.close_preview();
        self.close_group_view();
        self.hovered = None;
        self.hovering = false;
        self.render()?;
        Ok(Some(Outside::FolderMenu {
            folder,
            anchor,
            placement,
            dark: !crate::appearance::light(),
        }))
    }

    /// Where a menu for the icon at `index` opens: just off it, on the side
    /// away from the screen edge (see `Stage::menu_placement`).
    pub(super) fn anchor_beside(&self, index: usize) -> Option<POINT> {
        let stage = self.stage.as_ref()?;
        let state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value,
            &stage.magnifier,
        );
        let rect = state.rects.get(index)?;
        let gap = (stage.gap / 2.0).round() as i32;
        Some(stage.beside(*rect, gap))
    }

    /// Middle click on an app opens another window of it, as on the Windows
    /// taskbar.
    pub(super) fn on_middle_click(&mut self) -> Result<Option<Outside>> {
        self.close_preview();
        self.close_group_view();
        if self.pointer_at_click()?.is_none() {
            return Ok(None);
        }
        let Some(entry) = self.hovered.and_then(|index| self.entries.get(index)) else {
            return Ok(None);
        };
        if entry.role == Role::Window {
            if let Some(id) = &entry.id
                && let Some(hwnd) = window_of(id)
            {
                nosih_shell::tracker::request_close_window(hwnd);
            }
            return Ok(None);
        }
        let (Some(id), Role::App) = (entry.id.clone(), entry.role) else {
            return Ok(None);
        };
        self.start_bounce(id.clone(), None);
        Ok(Some(Outside::Open(id)))
    }

    /// Right-click prepares the menu for the item under the cursor, or the dock
    /// menu when over empty space or a divider.
    pub(super) fn on_right_click(&mut self) -> Result<Option<Outside>> {
        if self.menu_open {
            return Ok(None);
        }
        self.close_preview();
        self.close_group_view();
        if self.pointer_at_click()?.is_none() {
            return Ok(None);
        }
        let Some(stage) = &self.stage else {
            return Ok(None);
        };
        let state = magnify(
            &stage.layout,
            self.cursor,
            self.amount.value,
            &stage.magnifier,
        );
        let target_entry = self.hovered.and_then(|index| {
            let entry = self.entries.get(index)?;
            Some((entry.id.clone()?, entry.role, *state.rects.get(index)?))
        });
        let gap = (stage.gap / 2.0).round() as i32;
        let stage_placement = stage.menu_placement();

        let (target, items, anchor) = match target_entry {
            Some((id, role, rect)) => {
                let windows = self.windows_of(&id);
                let anchor = stage.beside(rect, gap);
                let (items, end_task, jump) = self.entry_menu(&id, role, &windows);
                (
                    MenuTarget::Entry {
                        id,
                        role,
                        windows,
                        end_task,
                        jump,
                    },
                    items,
                    anchor,
                )
            }
            None => {
                let anchor = stage.beside(
                    Rect {
                        x: self.cursor,
                        y: 0.0,
                        width: 0.0,
                        height: stage.layout.height,
                    },
                    gap,
                );
                let hiding = if self.config.hides_automatically() {
                    text().turn_hiding_off
                } else {
                    text().turn_hiding_on
                };
                let items = vec![
                    MenuItem::action(CMD_TOGGLE_HIDING, hiding),
                    MenuItem::action(CMD_SHOW_DESKTOP, text().show_desktop),
                    MenuItem::Separator,
                    MenuItem::action(CMD_TASK_MANAGER, text().task_manager),
                    MenuItem::action(CMD_TASKBAR_SETTINGS, text().taskbar_settings),
                    MenuItem::Separator,
                    MenuItem::action(CMD_SETTINGS, text().settings),
                    MenuItem::Separator,
                    MenuItem::action(CMD_QUIT_DOCK, text().quit_dock),
                ];
                (MenuTarget::Dock, items, anchor)
            }
        };

        Ok(Some(Outside::Menu(MenuRequest {
            gfx: self.gfx.clone(),
            target,
            items,
            anchor,
            placement: stage_placement,
            dark: !crate::appearance::light(),
        })))
    }

    /// The menu of a dock item, the app's prepared End Task and what its jump
    /// list entries open (see `CMD_JUMP`).
    pub(super) fn entry_menu(
        &self,
        id: &str,
        role: Role,
        windows: &[isize],
    ) -> (
        Vec<MenuItem>,
        Option<PreparedEndTask>,
        Vec<jumplist::Target>,
    ) {
        let is_file = !id.starts_with("shell:") && !id.starts_with("::");
        let t = text();
        let mut items = Vec::new();
        let mut jump = Vec::new();
        let end_task = if role == Role::App && id != explorer_id() {
            PreparedEndTask::capture(id, windows, &self.windows)
        } else {
            None
        };
        match role {
            Role::App => {
                // The app's jump list first, as on the taskbar.
                let groups = if self.config.jump_lists {
                    jumplist::for_dock_item(id, windows)
                } else {
                    Vec::new()
                };
                for group in groups {
                    items.push(MenuItem::Header(match group.section {
                        jumplist::Section::Pinned => t.jump_pinned.to_owned(),
                        jumplist::Section::Frequent => t.jump_frequent.to_owned(),
                        jumplist::Section::Recent => t.jump_recent.to_owned(),
                        jumplist::Section::Tasks => t.jump_tasks.to_owned(),
                        jumplist::Section::Custom(name) => shorten(&name, MENU_TITLE_CHARS),
                    }));
                    for entry in group.entries {
                        match entry {
                            jumplist::Entry::Item {
                                label,
                                target,
                                icon,
                            } => {
                                items.push(MenuItem::Action {
                                    command: CMD_JUMP + jump.len() as u32,
                                    label: shorten(&label, MENU_TITLE_CHARS).replace('&', "&&"),
                                    checked: false,
                                    enabled: true,
                                    icon,
                                });
                                jump.push(target);
                            }
                            jumplist::Entry::Separator => items.push(MenuItem::Separator),
                        }
                    }
                }
                if !items.is_empty() {
                    items.push(MenuItem::Separator);
                }
                let jump_rows = items.len();
                for (index, &hwnd) in windows.iter().take(MENU_WINDOWS).enumerate() {
                    let title = tracker::title(hwnd);
                    if !title.is_empty() {
                        items.push(MenuItem::action(
                            CMD_WINDOW + index as u32,
                            shorten(&title, MENU_TITLE_CHARS).replace('&', "&&"),
                        ));
                    }
                }
                if items.len() > jump_rows {
                    items.push(MenuItem::Separator);
                }
                let open = if windows.is_empty() {
                    t.open
                } else {
                    t.new_window
                };
                items.push(MenuItem::action(CMD_OPEN, open));
                if id != explorer_id() {
                    items.push(MenuItem::Action {
                        command: CMD_TOGGLE_PIN,
                        label: t.keep_in_dock.into(),
                        checked: self.roster.is_pinned(id),
                        enabled: true,
                        icon: None,
                    });
                }
                // A recent app can be dismissed while recent apps stay on.
                if windows.is_empty() && self.roster.recent().iter().any(|recent| recent == id) {
                    items.push(MenuItem::action(CMD_FORGET_RECENT, t.remove_from_dock));
                }
                if is_file {
                    items.push(MenuItem::action(CMD_RUN_AS_ADMIN, t.run_as_admin));
                    items.push(MenuItem::action(CMD_REVEAL, t.show_in_explorer));
                }
                if !windows.is_empty() {
                    items.push(MenuItem::Separator);
                    items.push(MenuItem::action(CMD_QUIT_APP, t.quit_app));
                    if id != explorer_id() {
                        items.push(MenuItem::Action {
                            command: CMD_END_TASK,
                            label: t.end_task.into(),
                            checked: false,
                            enabled: end_task.is_some(),
                            icon: None,
                        });
                    }
                }
            }
            Role::Group => {
                items.push(MenuItem::action(CMD_OPEN, t.open));
                items.push(MenuItem::Separator);
                items.push(MenuItem::action(CMD_UNGROUP, t.ungroup));
            }
            Role::Start => items.push(MenuItem::action(CMD_OPEN, t.open)),
            Role::Window => {
                items.push(MenuItem::action(CMD_OPEN, t.open));
                items.push(MenuItem::Separator);
                items.push(MenuItem::action(CMD_CLOSE_WINDOW, t.close_window));
            }
            Role::Folder => {
                items.push(MenuItem::action(CMD_OPEN, t.open));
                items.push(MenuItem::action(CMD_REVEAL, t.show_in_explorer));
            }
            // The folder is chosen in Settings, so changing it opens them.
            Role::Shortcuts => {
                items.push(MenuItem::action(CMD_OPEN, t.open));
                items.push(MenuItem::action(CMD_SETTINGS, t.change_folder));
                items.push(MenuItem::Separator);
                items.push(MenuItem::action(CMD_REMOVE_FOLDER, t.remove_from_dock));
            }
            Role::Trash => {
                items.push(MenuItem::action(CMD_OPEN, t.open));
                items.push(MenuItem::Separator);
                items.push(MenuItem::Action {
                    command: CMD_EMPTY_BIN,
                    label: t.empty_bin.into(),
                    checked: false,
                    enabled: self.bin_full,
                    icon: None,
                });
            }
        }
        if fixed_item_preference(role).is_some() {
            items.push(MenuItem::Separator);
            items.push(MenuItem::action(CMD_HIDE_FIXED, t.remove_from_dock));
        }
        (items, end_task, jump)
    }

    pub(super) fn on_command(
        &mut self,
        target: MenuTarget,
        command: u32,
    ) -> Result<Option<Outside>> {
        let MenuTarget::Entry {
            id,
            role,
            windows,
            end_task,
            jump,
        } = target
        else {
            return match command {
                CMD_QUIT_DOCK => Ok(Some(Outside::QuitDock)),
                CMD_TOGGLE_HIDING => self.toggle_hiding().map(|()| None),
                CMD_SHOW_DESKTOP => Ok(Some(Outside::ShowDesktop)),
                CMD_TASK_MANAGER => Ok(Some(Outside::Open("taskmgr.exe".into()))),
                CMD_TASKBAR_SETTINGS => Ok(Some(Outside::Open("ms-settings:taskbar".into()))),
                CMD_SETTINGS => Ok(Some(Outside::OpenSettings)),
                _ => Ok(None),
            };
        };
        Ok(match command {
            CMD_OPEN if role == Role::Group => {
                self.open_group_view(&id)?;
                None
            }
            // The group's apps go back to the dock, in its place.
            CMD_UNGROUP => {
                self.close_group_view();
                self.roster.dissolve(&id);
                save_settings(&self.roster, &self.config);
                self.refresh()?;
                None
            }
            CMD_OPEN if role == Role::Start => Some(Outside::OpenStart),
            CMD_OPEN if role == Role::Window => {
                window_of(&id).and_then(|hwnd| self.activating(vec![hwnd]))
            }
            CMD_CLOSE_WINDOW => {
                launcher::close(&window_of(&id).into_iter().collect::<Vec<_>>());
                None
            }
            CMD_SETTINGS => Some(Outside::OpenSettings),
            CMD_HIDE_FIXED => {
                let saved = hide_fixed_item(&nosih_config::default_path(), role)?;
                self.config.show_start = saved.show_start;
                self.config.show_downloads = saved.show_downloads;
                self.config.show_trash = saved.show_trash;
                self.refresh()?;
                None
            }
            CMD_FORGET_RECENT => {
                if self.roster.forget_recent(&id) {
                    save_settings(&self.roster, &self.config);
                    self.refresh()?;
                }
                None
            }
            CMD_REMOVE_FOLDER => {
                if let Err(error) = forget_shortcuts_folder(&self.config) {
                    eprintln!("nosih: {error}");
                }
                self.config.shortcuts_folder = None;
                self.refresh()?;
                None
            }
            CMD_OPEN => {
                if role == Role::App {
                    self.start_bounce(id.clone(), None);
                }
                Some(Outside::Open(id))
            }
            CMD_TOGGLE_PIN => {
                // Unpinning here also unpins on the Windows taskbar, as there is
                // only one taskbar for the user. Pinning cannot go the other way.
                let on_taskbar = self
                    .config
                    .taskbar_pins
                    .as_ref()
                    .is_some_and(|pins| pins.contains(&id));
                let unpinning = self.roster.is_pinned(&id);
                if unpinning {
                    self.roster.unpin(&id);
                    pins::forget_shortcut(&id);
                } else {
                    self.roster.pin(&id);
                }
                save_settings(&self.roster, &self.config);
                self.refresh()?;
                (unpinning && on_taskbar).then_some(Outside::UnpinFromTaskbar(id))
            }
            CMD_RUN_AS_ADMIN => Some(Outside::OpenAsAdmin(id)),
            CMD_REVEAL => Some(Outside::Reveal(id)),
            CMD_QUIT_APP => {
                launcher::close(&windows);
                None
            }
            CMD_END_TASK => end_task.map(Outside::EndTask),
            CMD_EMPTY_BIN => Some(Outside::EmptyBin),
            command if command >= CMD_JUMP => {
                let target = jump.into_iter().nth((command - CMD_JUMP) as usize);
                target.map(|target| {
                    self.start_bounce(id.clone(), None);
                    Outside::OpenJump {
                        target,
                        program: id,
                    }
                })
            }
            window => {
                let index = window.checked_sub(CMD_WINDOW).map(|index| index as usize);
                index
                    .and_then(|index| windows.get(index))
                    .and_then(|&hwnd| self.activating(vec![hwnd]))
            }
        })
    }
}
