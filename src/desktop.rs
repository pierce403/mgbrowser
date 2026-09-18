//! Native desktop composition. Engine state remains in Chassis.
use crate::{
    platform,
    settings::{Preferences, Settings},
    updater,
    workspace::{Limits, PaneId, Removed, Side, TabId, WindowId, Workspace},
};
use mg_chassis::{Browser as App, BrowserCdp, ColorScheme};
use mg_sparkle::paint::{Canvas, Fonts};
use std::{
    collections::BTreeMap,
    error::Error,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

pub struct Options {
    pub initial: String,
    pub restore_tabs: Vec<String>,
    pub executable: PathBuf,
    pub scripts: bool,
    pub auto_update: bool,
    pub debug_port: Option<u16>,
    pub session: mg_chassis::net::Session,
}

const BG: u32 = 0xfafbf8;
const STRIP: u32 = 34;
const DIVIDER: u32 = 6;

#[derive(Clone, Copy)]
struct NativeWindow {
    xid: u32,
    gc: u32,
    width: u32,
    height: u32,
    scale: u16,
    theme: Option<ColorScheme>,
    dirty: bool,
}

struct Atoms {
    protocols: u32,
    close: u32,
    theme: u32,
    utf8: u32,
    icon: u32,
    icon_data: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DropTarget {
    Strip(PaneId, usize),
    Edge(WindowId, Side),
}

struct Drag {
    tab: TabId,
    start: (i16, i16),
    moving: bool,
    target: Option<DropTarget>,
}

#[derive(Clone, Copy)]
struct PaneRect {
    pane: PaneId,
    x: u32,
    width: u32,
    top: u32,
    height: u32,
}

struct Desktop {
    space: Workspace<App>,
    root: u32,
    native: BTreeMap<WindowId, NativeWindow>,
    initial_tab: TabId,
    strip: bool,
    drag: Option<Drag>,
    fonts: Fonts,
    system_theme: ColorScheme,
    system_scale: u16,
    scripts: bool,
    session: mg_chassis::net::Session,
    notice: Option<(String, Instant)>,
    titles: BTreeMap<TabId, String>,
}

impl Desktop {
    fn create_native(
        &self,
        conn: &RustConnection,
        atoms: &Atoms,
        screen_num: usize,
        size: (u32, u32),
        position: (i16, i16),
    ) -> Result<NativeWindow, Box<dyn Error>> {
        let screen = &conn.setup().roots[screen_num];
        let window = conn.generate_id()?;
        let gc = conn.generate_id()?;
        conn.create_window(
            screen.root_depth,
            window,
            screen.root,
            position.0.clamp(
                0,
                u32::from(screen.width_in_pixels)
                    .saturating_sub(size.0)
                    .min(i16::MAX as u32) as i16,
            ),
            position.1.clamp(
                0,
                u32::from(screen.height_in_pixels)
                    .saturating_sub(size.1)
                    .min(i16::MAX as u32) as i16,
            ),
            size.0 as u16,
            size.1 as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(BG).event_mask(
                EventMask::EXPOSURE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::KEY_PRESS
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::FOCUS_CHANGE,
            ),
        )?
        .check()?;
        let mut gc_created = false;
        let configure = (|| -> Result<(), Box<dyn Error>> {
            conn.change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                b"mgbrowser\0mgbrowser\0",
            )?;
            conn.change_property32(
                PropMode::REPLACE,
                window,
                atoms.protocols,
                AtomEnum::ATOM,
                &[atoms.close],
            )?;
            conn.change_property32(
                PropMode::REPLACE,
                window,
                atoms.icon,
                AtomEnum::CARDINAL,
                &atoms.icon_data,
            )?;
            conn.create_gc(gc, window, &CreateGCAux::new())?.check()?;
            gc_created = true;
            conn.map_window(window)?.check()?;
            conn.flush()?;
            Ok(())
        })();
        if let Err(error) = configure {
            if gc_created {
                let _ = conn.free_gc(gc);
            }
            let _ = conn.destroy_window(window);
            return Err(error);
        }
        eprintln!("WINDOW id={window} pid={}", std::process::id());
        Ok(NativeWindow {
            xid: window,
            gc,
            width: size.0,
            height: size.1,
            scale: 100,
            theme: None,
            dirty: true,
        })
    }

    fn window_id(&self, xid: u32) -> Option<WindowId> {
        self.native
            .iter()
            .find_map(|(&id, window)| (window.xid == xid).then_some(id))
    }

    fn active_in(&self, window: WindowId) -> TabId {
        let state = self.space.window(window).unwrap();
        self.space.pane(state.focused_pane()).unwrap().active()
    }

    fn rects(&self, window: WindowId) -> Vec<PaneRect> {
        let native = self.native[&window];
        let groups = self.space.window(window).unwrap().panes();
        let top = if self.strip {
            scaled_dimension(STRIP, native.scale)
        } else {
            0
        };
        let divider = if groups.len() == 2 {
            scaled_dimension(DIVIDER, native.scale)
        } else {
            0
        };
        let available = native.width.saturating_sub(divider);
        groups
            .iter()
            .enumerate()
            .map(|(index, pane)| {
                let x = if index == 0 {
                    0
                } else {
                    available / 2 + divider
                };
                let width = if groups.len() == 1 {
                    native.width
                } else if index == 0 {
                    available / 2
                } else {
                    available - available / 2
                };
                PaneRect {
                    pane: pane.id(),
                    x,
                    width,
                    top,
                    height: native.height.saturating_sub(top),
                }
            })
            .collect()
    }

    fn dirty_all(&mut self) {
        for window in self.native.values_mut() {
            window.dirty = true;
        }
    }

    fn notify(&mut self, message: impl Into<String>) {
        let message = message.into();
        eprintln!("WORKSPACE: {message}");
        self.notice = Some((message, Instant::now() + Duration::from_secs(6)));
        self.dirty_all();
    }

    fn cancel_input(&mut self) {
        for (_, app) in self.space.tabs_mut() {
            app.cancel_pointer_input();
        }
    }

    fn focus_window(&mut self, window: WindowId) -> Result<(), crate::workspace::Error> {
        let pane = self
            .space
            .window(window)
            .ok_or(crate::workspace::Error::UnknownWindow)?
            .focused_pane();
        // A release delivered after native focus moves must not activate a press
        // in the old page. Same-window grab/ungrab notifications preserve it.
        if self.space.focused_window() != Some(window) {
            self.cancel_input();
        }
        self.space.focus_pane(pane)
    }

    fn cancel_drag(&mut self, conn: &RustConnection) -> Result<(), Box<dyn Error>> {
        if self.drag.take().is_some() {
            conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
            self.dirty_all();
        }
        self.cancel_input();
        Ok(())
    }

    fn sync_geometry(&mut self, conn: &RustConnection) -> Result<(), Box<dyn Error>> {
        let count = self.space.tabs().count();
        for (_, app) in self.space.tabs_mut() {
            app.set_restart_tab_count(count);
        }
        let ids = self.native.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let scale = self
                .space
                .get(self.active_in(id))
                .unwrap()
                .effective_scale_percent();
            let groups = self.space.window(id).unwrap().panes().len() as u32;
            let native = self.native.get_mut(&id).unwrap();
            native.scale = scale;
            let min_width = scaled_dimension(360 * groups + DIVIDER * (groups - 1), scale);
            let min_height = scaled_dimension(240 + if self.strip { STRIP } else { 0 }, scale);
            // Real split groups must each retain Chassis's complete 360px toolbar.
            if groups > 1 && (native.width < min_width || native.height < min_height) {
                native.width = native.width.max(min_width).min(4800);
                native.height = native.height.max(min_height).min(3600);
                conn.configure_window(
                    native.xid,
                    &ConfigureWindowAux::new()
                        .width(native.width)
                        .height(native.height),
                )?;
            }
            x11rb::properties::WmSizeHints {
                min_size: Some((min_width as i32, min_height as i32)),
                max_size: Some((4800, 3600)),
                ..Default::default()
            }
            .set_normal_hints(conn, native.xid)?;
            native.dirty = true;
            for rect in self.rects(id) {
                let tabs = self.space.pane(rect.pane).unwrap().tabs().to_vec();
                for tab in tabs {
                    self.space
                        .get_mut(tab)
                        .unwrap()
                        .resize_surface(rect.width, rect.height);
                }
            }
            eprintln!(
                "TABS window={} groups={} count={count}",
                self.native[&id].xid, groups
            );
        }
        Ok(())
    }

    fn fresh_tab(
        &mut self,
        pane: PaneId,
        url: Option<String>,
        bookmarks: &crate::bookmarks::BookmarkStore,
        conn: &RustConnection,
    ) -> Result<(), Box<dyn Error>> {
        if self.space.tabs().count() >= self.space.limits().tabs {
            self.notify("Maximum16 tabs reached; close a tab first");
            return Ok(());
        }
        let fonts = match platform::load_fonts() {
            Ok(fonts) => fonts,
            Err(error) => {
                self.notify(format!("Cannot create tab: {error}"));
                return Ok(());
            }
        };
        let active = self.space.get(self.space.active_tab().unwrap()).unwrap();
        let mut app = App::new_with_session(
            fonts,
            std::sync::Arc::new(platform::LinuxScripts::default()),
            self.session.clone(),
        );
        app.set_build_info(env!("CARGO_PKG_VERSION"), crate::COMPILED, crate::REVISION);
        app.set_theme_preference(active.theme_preference());
        app.set_scale_preference(active.scale_preference());
        app.set_system_theme(self.system_theme);
        app.set_system_scale(self.system_scale);
        app.set_scripts_enabled(self.scripts);
        match bookmarks.load() {
            Ok(entries) => app.set_bookmarks(entries),
            Err(error) => app.set_bookmark_status(error),
        }
        if let Some(url) = url {
            app.navigate(url, None, true);
        }
        self.cancel_drag(conn)?;
        match self.space.open_tab(pane, app) {
            Ok(tab) => {
                self.strip = true;
                eprintln!("TAB opened={} pane={}", tab.get(), pane.get());
                self.sync_geometry(conn)?;
            }
            Err(error) => self.notify(error.error.to_string()),
        }
        Ok(())
    }

    fn cleanup(&mut self, removed: Removed, conn: &RustConnection) -> Result<(), Box<dyn Error>> {
        if let Some(id) = removed.window
            && let Some(native) = self.native.remove(&id)
        {
            conn.destroy_window(native.xid)?;
            conn.free_gc(native.gc)?;
        }
        Ok(())
    }

    fn close_tab(&mut self, tab: TabId, conn: &RustConnection) -> Result<(), Box<dyn Error>> {
        self.cancel_drag(conn)?;
        let closed = self.space.close_tab(tab)?;
        self.cleanup(closed.removed, conn)?;
        drop(closed.payload);
        if !self.space.is_empty() {
            self.sync_geometry(conn)?;
        }
        Ok(())
    }

    fn close_window(
        &mut self,
        id: WindowId,
        destroyed: bool,
        conn: &RustConnection,
    ) -> Result<(), Box<dyn Error>> {
        self.cancel_drag(conn)?;
        self.space.close_window(id)?;
        if let Some(window) = self.native.remove(&id) {
            if !destroyed {
                conn.destroy_window(window.xid)?;
            }
            conn.free_gc(window.gc)?;
        }
        if !self.space.is_empty() {
            self.sync_geometry(conn)?;
        }
        Ok(())
    }

    fn select(&mut self, tab: TabId) -> Result<(), Box<dyn Error>> {
        self.cancel_input();
        self.space.select_tab(tab)?;
        self.space.get_mut(tab).unwrap().request_redraw();
        self.dirty_all();
        Ok(())
    }

    fn strip_hit(&self, window: WindowId, x: i16, y: i16) -> Option<(PaneId, Option<TabId>, bool)> {
        if !self.strip || x < 0 || y < 0 {
            return None;
        }
        let native = self.native[&window];
        let rect = self.rects(window).into_iter().find(|rect| {
            u32::from(x as u16) >= rect.x && u32::from(x as u16) < rect.x + rect.width
        })?;
        if u32::from(y as u16) >= rect.top {
            return None;
        }
        let local_x =
            logical_coordinate((i32::from(x) - rect.x as i32) as i16, native.scale).max(0) as u32;
        let width = ((u64::from(rect.width) * 100).div_ceil(u64::from(native.scale))) as u32;
        if local_x >= width.saturating_sub(30) {
            return Some((rect.pane, None, false));
        }
        let pane = self.space.pane(rect.pane).unwrap();
        let tab_width = (width.saturating_sub(30) / pane.tabs().len() as u32).clamp(1, 200);
        let tab = *pane.tabs().get((local_x / tab_width) as usize)?;
        Some((
            rect.pane,
            Some(tab),
            tab_width >= 52 && local_x % tab_width >= tab_width - 18,
        ))
    }

    fn page_hit(&self, window: WindowId, x: i16, y: i16) -> Option<(PaneId, TabId, i32, i32)> {
        if x < 0 || y < 0 {
            return None;
        }
        let scale = self.native[&window].scale;
        self.rects(window).into_iter().find_map(|rect| {
            if u32::from(x as u16) < rect.x
                || u32::from(x as u16) >= rect.x + rect.width
                || u32::from(y as u16) < rect.top
            {
                return None;
            }
            Some((
                rect.pane,
                self.space.pane(rect.pane).unwrap().active(),
                logical_coordinate((i32::from(x) - rect.x as i32) as i16, scale),
                logical_coordinate((i32::from(y) - rect.top as i32) as i16, scale),
            ))
        })
    }

    fn drag_target(
        &self,
        root: (i16, i16),
        conn: &RustConnection,
    ) -> Result<Option<DropTarget>, Box<dyn Error>> {
        let stacking = conn.query_tree(self.root)?.reply()?.children;
        let mut candidates = Vec::new();
        for (&id, window) in &self.native {
            let mut ancestor = window.xid;
            for _ in 0..32 {
                let tree = conn.query_tree(ancestor)?.reply()?;
                if tree.parent == self.root || tree.parent == x11rb::NONE {
                    break;
                }
                ancestor = tree.parent;
            }
            candidates.push((
                stacking
                    .iter()
                    .position(|&xid| xid == ancestor)
                    .unwrap_or(0),
                id,
                *window,
            ));
        }
        candidates.sort_by_key(|(position, _, _)| *position);
        for (_, id, window) in candidates.into_iter().rev() {
            let translated = conn
                .translate_coordinates(self.root, window.xid, root.0, root.1)?
                .reply()?;
            let (x, y) = (translated.dst_x, translated.dst_y);
            if x < 0 || y < 0 || x as u32 >= window.width || y as u32 >= window.height {
                continue;
            }
            if self.strip && (y as u32) < scaled_dimension(STRIP, window.scale) {
                let rect = self
                    .rects(id)
                    .into_iter()
                    .find(|rect| x as u32 >= rect.x && (x as u32) < rect.x + rect.width);
                if let Some(rect) = rect {
                    let pane = self.space.pane(rect.pane).unwrap();
                    let width =
                        ((u64::from(rect.width) * 100).div_ceil(u64::from(window.scale))) as u32;
                    let tab_width =
                        (width.saturating_sub(30) / pane.tabs().len() as u32).clamp(1, 200);
                    let local =
                        logical_coordinate((i32::from(x) - rect.x as i32) as i16, window.scale)
                            .max(0) as u32;
                    return Ok(Some(DropTarget::Strip(
                        rect.pane,
                        (local / tab_width) as usize,
                    )));
                }
            }
            let edge = scaled_dimension(80, window.scale);
            if (x as u32) < edge {
                return Ok(Some(DropTarget::Edge(id, Side::Left)));
            }
            if x as u32 >= window.width.saturating_sub(edge) {
                return Ok(Some(DropTarget::Edge(id, Side::Right)));
            }
            // A release over page content is cancellation, not detachment.
            return Ok(Some(DropTarget::Strip(
                self.space.window(id).unwrap().focused_pane(),
                usize::MAX,
            )));
        }
        Ok(None)
    }

    fn press(
        &mut self,
        window: WindowId,
        e: ButtonPressEvent,
        conn: &RustConnection,
        bookmarks: &crate::bookmarks::BookmarkStore,
    ) -> Result<(), Box<dyn Error>> {
        if self.drag.is_some() {
            self.cancel_drag(conn)?;
        }
        if e.detail == 1
            && let Some((pane, tab, close)) = self.strip_hit(window, e.event_x, e.event_y)
        {
            if let Some(tab) = tab {
                if close {
                    return self.close_tab(tab, conn);
                }
                self.select(tab)?;
                let grab = conn
                    .grab_pointer(
                        false,
                        self.native[&window].xid,
                        EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        x11rb::NONE,
                        x11rb::NONE,
                        e.time,
                    )?
                    .reply()?;
                if grab.status == GrabStatus::SUCCESS {
                    self.drag = Some(Drag {
                        tab,
                        start: (e.root_x, e.root_y),
                        moving: false,
                        target: None,
                    });
                } else {
                    self.notify("Cannot capture pointer for tab drag; tab remains selected");
                }
            } else {
                self.fresh_tab(pane, None, bookmarks, conn)?;
            }
            return Ok(());
        }
        if let Some((pane, tab, x, y)) = self.page_hit(window, e.event_x, e.event_y) {
            if self.space.active_tab() != Some(tab) {
                self.cancel_input();
                self.space.focus_pane(pane)?;
                self.dirty_all();
            }
            let app = self.space.get_mut(tab).unwrap();
            match e.detail {
                1 => app.pointer_down(x, y),
                3 => app.context_menu_at(x, y),
                4 => app.wheel_scroll_by(-100),
                5 => app.wheel_scroll_by(100),
                _ => {}
            }
        }
        Ok(())
    }

    fn motion(&mut self, root: (i16, i16), conn: &RustConnection) -> Result<(), Box<dyn Error>> {
        let Some(drag) = &self.drag else {
            return Ok(());
        };
        let Some(app) = self.space.get(drag.tab) else {
            return self.cancel_drag(conn);
        };
        let scale = app.effective_scale_percent();
        let threshold = scaled_dimension(8, scale) as i32;
        let moving = drag.moving
            || (i32::from(root.0) - i32::from(drag.start.0))
                .abs()
                .max((i32::from(root.1) - i32::from(drag.start.1)).abs())
                >= threshold;
        let target = if moving {
            self.drag_target(root, conn)?
        } else {
            None
        };
        let drag = self.drag.as_mut().unwrap();
        if drag.moving != moving || drag.target != target {
            drag.moving = moving;
            drag.target = target;
            self.dirty_all();
        }
        Ok(())
    }

    fn release(
        &mut self,
        window: WindowId,
        e: ButtonReleaseEvent,
        conn: &RustConnection,
        atoms: &Atoms,
        screen_num: usize,
    ) -> Result<(), Box<dyn Error>> {
        if e.detail != 1 {
            return Ok(());
        }
        if self.drag.is_some() {
            self.motion((e.root_x, e.root_y), conn)?;
            let Some(drag) = self.drag.take() else {
                return Ok(());
            };
            conn.ungrab_pointer(e.time)?;
            self.dirty_all();
            if !drag.moving {
                return Ok(());
            }
            let result = match drag.target {
                Some(DropTarget::Strip(_, usize::MAX)) => return Ok(()),
                Some(DropTarget::Strip(pane, index)) => {
                    let source = self.space.location(drag.tab).unwrap();
                    let len = self.space.pane(pane).unwrap().tabs().len()
                        - usize::from(source.pane == pane);
                    self.space
                        .move_tab(drag.tab, pane, index.min(len))
                        .map(|removed| (None, removed))
                }
                Some(DropTarget::Edge(id, side)) => self
                    .space
                    .dock_tab(drag.tab, id, side)
                    .map(|(_, removed)| (None, removed)),
                None => match self.space.prepare_detach(drag.tab) {
                    Ok(plan) => {
                        let source = self.native[&self.space.location(drag.tab).unwrap().window];
                        match self.create_native(
                            conn,
                            atoms,
                            screen_num,
                            (source.width, source.height),
                            (e.root_x, e.root_y),
                        ) {
                            Ok(native) => match self.space.commit_detach(plan) {
                                Ok((opened, removed)) => {
                                    Ok((Some((opened.window, native)), removed))
                                }
                                Err(error) => {
                                    conn.destroy_window(native.xid)?;
                                    conn.free_gc(native.gc)?;
                                    Err(error)
                                }
                            },
                            Err(error) => {
                                self.notify(format!("Cannot detach tab: {error}"));
                                return Ok(());
                            }
                        }
                    }
                    Err(error) => Err(error),
                },
            };
            match result {
                Ok((new_window, removed)) => {
                    if let Some((id, native)) = new_window {
                        self.native.insert(id, native);
                    }
                    self.cleanup(removed, conn)?;
                    self.cancel_input();
                    self.sync_geometry(conn)?;
                }
                Err(error) => self.notify(error.to_string()),
            }
        } else if let Some((_, tab, x, y)) = self.page_hit(window, e.event_x, e.event_y) {
            self.space.get_mut(tab).unwrap().pointer_up(x, y);
        }
        Ok(())
    }

    fn key(
        &mut self,
        window: WindowId,
        key: mg_chassis::Key,
        ctrl: bool,
        shift: bool,
        alt: bool,
        conn: &RustConnection,
        bookmarks: &crate::bookmarks::BookmarkStore,
    ) -> Result<(), Box<dyn Error>> {
        if matches!(key, mg_chassis::Key::Escape) && self.drag.take().is_some() {
            conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
            self.cancel_input();
            self.dirty_all();
            return Ok(());
        }
        if self.drag.is_some() {
            self.cancel_drag(conn)?;
        }
        let tab = self.active_in(window);
        let pane = self.space.location(tab).unwrap().pane;
        if ctrl && !alt {
            match key {
                mg_chassis::Key::Character('t' | 'T') => {
                    return self.fresh_tab(pane, None, bookmarks, conn);
                }
                mg_chassis::Key::Character('w' | 'W') => return self.close_tab(tab, conn),
                mg_chassis::Key::Tab => {
                    let group = self.space.pane(pane).unwrap();
                    let index = group.tabs().iter().position(|&id| id == tab).unwrap();
                    let next = if shift {
                        (index + group.tabs().len() - 1) % group.tabs().len()
                    } else {
                        (index + 1) % group.tabs().len()
                    };
                    return self.select(group.tabs()[next]);
                }
                _ => {}
            }
        }
        self.space
            .get_mut(tab)
            .unwrap()
            .handle_key(key, ctrl, shift, alt);
        Ok(())
    }

    fn paint(&mut self, conn: &RustConnection, depth: u8) -> Result<(), Box<dyn Error>> {
        let changed = self
            .space
            .tabs()
            .map(|(id, app)| (id, app.visible_title()))
            .collect::<BTreeMap<_, _>>();
        if self.titles != changed {
            self.titles = changed;
            self.dirty_all();
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.notice = None;
            self.dirty_all();
        }
        let windows = self.native.keys().copied().collect::<Vec<_>>();
        for id in windows {
            let native = self.native[&id];
            let rects = self.rects(id);
            let mut drew = false;
            for rect in &rects {
                let tab = self.space.pane(rect.pane).unwrap().active();
                let app = self.space.get_mut(tab).unwrap();
                app.advance_scroll(Instant::now());
                let dirty = app.is_dirty() || native.dirty;
                if dirty {
                    let canvas = app.paint();
                    upload(
                        conn,
                        native,
                        depth,
                        &canvas,
                        (rect.x, rect.top),
                        (rect.width, rect.height),
                    )?;
                    app.advance_journey(&canvas);
                    drew = true;
                } else if app.journey_needs_redraw() {
                    app.request_redraw();
                }
                if self.strip && dirty {
                    self.paint_strip(id, *rect, conn, depth)?;
                }
            }
            if rects.len() == 2 && (native.dirty || drew) {
                let divider = Canvas::new(
                    scaled_dimension(DIVIDER, native.scale),
                    native.height,
                    0x59675b,
                );
                upload(
                    conn,
                    native,
                    depth,
                    &divider,
                    (rects[0].width, 0),
                    (divider.width, native.height),
                )?;
            }
            if native.dirty || drew {
                let title = format!(
                    "{} : mgbrowser",
                    self.space.get(self.active_in(id)).unwrap().visible_title()
                );
                conn.change_property8(
                    PropMode::REPLACE,
                    native.xid,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    title.as_bytes(),
                )?;
                if let Some((message, _)) = &self.notice {
                    let message = message.clone();
                    self.paint_label(
                        id,
                        0,
                        native
                            .height
                            .saturating_sub(scaled_dimension(29, native.scale)),
                        native.width,
                        &message,
                        0x9f202b,
                        0xffffff,
                        conn,
                        depth,
                    )?;
                }
                if let Some(drag) = &self.drag
                    && drag.moving
                {
                    let preview = match drag.target {
                        Some(DropTarget::Edge(window, side)) if window == id => Some(match side {
                            Side::Left => "Drop: split left",
                            Side::Right => "Drop: split right",
                        }),
                        Some(DropTarget::Strip(pane, index))
                            if index != usize::MAX
                                && self
                                    .space
                                    .window(id)
                                    .unwrap()
                                    .panes()
                                    .iter()
                                    .any(|group| group.id() == pane) =>
                        {
                            Some("Drop: move tab here")
                        }
                        None if self
                            .space
                            .location(drag.tab)
                            .is_some_and(|location| location.window == id) =>
                        {
                            Some("Release outside to detach; Escape cancels")
                        }
                        _ => None,
                    };
                    if let Some(label) = preview {
                        self.paint_label(
                            id,
                            0,
                            scaled_dimension(STRIP, native.scale),
                            native.width,
                            label,
                            0x345c36,
                            0xffffff,
                            conn,
                            depth,
                        )?;
                    }
                }
                self.native.get_mut(&id).unwrap().dirty = false;
            }
        }
        conn.flush()?;
        Ok(())
    }

    fn paint_strip(
        &mut self,
        id: WindowId,
        rect: PaneRect,
        conn: &RustConnection,
        depth: u8,
    ) -> Result<(), Box<dyn Error>> {
        let native = self.native[&id];
        let pane = self.space.pane(rect.pane).unwrap();
        let dark = self.space.get(pane.active()).unwrap().effective_theme() == ColorScheme::Dark;
        let (background, button, ink, accent) = if dark {
            (0x202622, 0x323c35, 0xe7eee6, 0xa7dba6)
        } else {
            (0xeef1e9, 0xe3e9df, 0x26342b, 0x345c36)
        };
        let width = ((u64::from(rect.width) * 100).div_ceil(u64::from(native.scale))) as u32;
        let mut canvas = Canvas::new_scaled(
            width.max(1),
            STRIP,
            background,
            f32::from(native.scale) / 100.,
        );
        canvas.rect(0, 0, width, STRIP, background);
        let tab_width = (width.saturating_sub(30) / pane.tabs().len() as u32).clamp(1, 200);
        for (index, &tab) in pane.tabs().iter().enumerate() {
            let x = index as i32 * tab_width as i32;
            canvas.rect(
                x + 1,
                2,
                tab_width.saturating_sub(2),
                STRIP - 3,
                if tab == pane.active() {
                    button
                } else {
                    background
                },
            );
            if tab == pane.active() {
                canvas.rect(
                    x + 1,
                    STRIP as i32 - 3,
                    tab_width.saturating_sub(2),
                    2,
                    accent,
                );
            }
            let room = tab_width.saturating_sub(if tab_width >= 52 { 30 } else { 10 });
            let title = fitted(
                &mut self.fonts,
                &self
                    .titles
                    .get(&tab)
                    .cloned()
                    .unwrap_or_else(|| "New tab".into()),
                room as f32,
                13.,
            );
            canvas.text(&mut self.fonts, x + 6, 9, &title, 13., ink);
            if tab_width >= 52 {
                canvas.text(&mut self.fonts, x + tab_width as i32 - 15, 8, "×", 15., ink);
            }
        }
        canvas.text(
            &mut self.fonts,
            width.saturating_sub(23) as i32,
            6,
            "+",
            20.,
            ink,
        );
        upload(
            conn,
            native,
            depth,
            &canvas,
            (rect.x, 0),
            (rect.width, rect.top),
        )
    }

    fn paint_label(
        &mut self,
        id: WindowId,
        x: u32,
        y: u32,
        width: u32,
        text: &str,
        background: u32,
        ink: u32,
        conn: &RustConnection,
        depth: u8,
    ) -> Result<(), Box<dyn Error>> {
        let native = self.native[&id];
        let logical_width = ((u64::from(width) * 100).div_ceil(u64::from(native.scale))) as u32;
        let mut canvas = Canvas::new_scaled(
            logical_width.max(1),
            29,
            background,
            f32::from(native.scale) / 100.,
        );
        canvas.rect(0, 0, logical_width, 29, background);
        let text = fitted(
            &mut self.fonts,
            text,
            logical_width.saturating_sub(16) as f32,
            13.,
        );
        canvas.text(&mut self.fonts, 8, 7, &text, 13., ink);
        upload(
            conn,
            native,
            depth,
            &canvas,
            (x, y),
            (width, native.height.saturating_sub(y)),
        )
    }
}

fn fitted(fonts: &mut Fonts, text: &str, width: f32, size: f32) -> String {
    let mut text = text.chars().take(256).collect::<String>();
    while !text.is_empty() && fonts.width(&text, size) > width {
        text.pop();
    }
    text
}

fn membership_changed<T>(previous: &mut Vec<TabId>, space: &Workspace<T>) -> bool {
    let current = space.tabs().map(|(id, _)| id).collect::<Vec<_>>();
    if *previous == current {
        return false;
    }
    *previous = current;
    true
}

fn upload(
    conn: &RustConnection,
    window: NativeWindow,
    depth: u8,
    canvas: &Canvas,
    offset: (u32, u32),
    clip: (u32, u32),
) -> Result<(), Box<dyn Error>> {
    let width = canvas.width.min(clip.0);
    let height = canvas.height.min(clip.1);
    if width == 0 || height == 0 {
        return Ok(());
    }
    let rows = (200_000 / (width as usize * 4)).max(1);
    for first in (0..height as usize).step_by(rows) {
        let end = (first + rows).min(height as usize);
        let mut data = Vec::with_capacity((end - first) * width as usize * 4);
        for row in first..end {
            let start = row * canvas.width as usize;
            data.extend(
                canvas.pixels[start..start + width as usize]
                    .iter()
                    .flat_map(|pixel| pixel.to_le_bytes()),
            );
        }
        conn.put_image(
            ImageFormat::Z_PIXMAP,
            window.xid,
            window.gc,
            width as u16,
            (end - first) as u16,
            offset.0 as i16,
            (offset.1 + first as u32) as i16,
            0,
            depth,
            &data,
        )?;
    }
    Ok(())
}

fn scaled_dimension(logical: u32, percent: u16) -> u32 {
    ((u64::from(logical) * u64::from(percent.clamp(75, 300)) + 50) / 100).min(u64::from(u32::MAX))
        as u32
}

fn logical_coordinate(physical: i16, percent: u16) -> i32 {
    // Floor, rather than truncate, so a negative coordinate stays outside.
    (i32::from(physical) * 100).div_euclid(i32::from(percent.clamp(75, 300)))
}

fn set_size_hints<C: Connection>(
    conn: &C,
    window: Window,
    percent: u16,
) -> Result<(), Box<dyn Error>> {
    x11rb::properties::WmSizeHints {
        min_size: Some((
            scaled_dimension(360, percent) as i32,
            scaled_dimension(240, percent) as i32,
        )),
        max_size: Some((4800, 3600)),
        ..Default::default()
    }
    .set_normal_hints(conn, window)?;
    Ok(())
}

pub fn run(mut app: App, options: Options) -> Result<(), Box<dyn Error>> {
    let Options {
        initial,
        restore_tabs,
        executable,
        scripts: restart_scripts,
        auto_update,
        debug_port,
        session,
    } = options;
    let settings = Settings::user();
    let bookmarks = crate::bookmarks::BookmarkStore::user();
    match bookmarks.load() {
        Ok(entries) => app.set_bookmarks(entries),
        Err(error) => app.set_bookmark_status(error),
    }
    match settings.load() {
        Ok(preferences) => {
            app.set_theme_preference(preferences.theme);
            app.set_scale_preference(preferences.scale);
        }
        Err(error) => {
            eprintln!("SETTINGS: {error}");
            app.set_settings_status(error);
        }
    }
    let (update_tx, update_rx) = std::sync::mpsc::channel();
    let mut update_running = false;
    let mut restart_ready = false;
    let mut next_update = std::time::Instant::now();
    app.set_update_status(
        if auto_update {
            "Automatic updates enabled"
        } else {
            "Automatic checks off; use Check for updates"
        }
        .into(),
    );
    let mut cdp = debug_port.map(BrowserCdp::bind).transpose()?;
    app.set_debugging(cdp.is_some());
    let (conn, screen_num) = x11rb::connect(None).map_err(|error| {
        format!("Cannot open X11 display: {error}. Run inside an X11 or XWayland desktop with DISPLAY set.")
    })?;
    let screen = &conn.setup().roots[screen_num];
    let system_scale = platform::scaling::read_scale(&conn, screen_num).unwrap_or(100);
    app.set_system_scale(system_scale);
    let applied_scale = app.effective_scale_percent();
    let surface_width = scaled_dimension(app.width(), applied_scale)
        .min(
            u32::from(screen.width_in_pixels)
                .saturating_sub(80)
                .max(360),
        )
        .min(4800);
    let surface_height = scaled_dimension(app.height(), applied_scale)
        .min(
            u32::from(screen.height_in_pixels)
                .saturating_sub(80)
                .max(240),
        )
        .min(3600);
    app.resize_surface(surface_width, surface_height);
    let mut next_scale_check = std::time::Instant::now() + Duration::from_secs(2);
    eprintln!("DISPLAY_SCALE system={system_scale}% effective={applied_scale}%");
    let depth = screen.root_depth;
    let format = conn
        .setup()
        .pixmap_formats
        .iter()
        .find(|f| f.depth == depth)
        .ok_or("Unsupported X11 visual")?;
    if format.bits_per_pixel != 32 || conn.setup().image_byte_order != ImageOrder::LSB_FIRST {
        return Err("Initial window backend requires a 32-bit little-endian pixel surface".into());
    }
    let window = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_window(
        depth,
        window,
        screen.root,
        40,
        40,
        surface_width as u16,
        surface_height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().background_pixel(BG).event_mask(
            EventMask::EXPOSURE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::KEY_PRESS
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION
                | EventMask::FOCUS_CHANGE,
        ),
    )?;
    set_size_hints(&conn, window, applied_scale)?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"mgbrowser - research browser",
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"mgbrowser\0mgbrowser\0",
    )?;
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let theme_atom = conn
        .intern_atom(false, b"_GTK_THEME_VARIANT")?
        .reply()?
        .atom;
    let utf8_atom = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let system_themes = platform::appearance::spawn_watcher();
    let window_theme = None;
    let icon_atom = conn.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
    let icon = image::load_from_memory_with_format(
        include_bytes!("../assets/mgbrowser-32.png"),
        image::ImageFormat::Png,
    )?
    .into_rgba8();
    let mut icon_data = vec![icon.width(), icon.height()];
    icon_data.extend(icon.pixels().map(|pixel| {
        (u32::from(pixel[3]) << 24)
            | (u32::from(pixel[0]) << 16)
            | (u32::from(pixel[1]) << 8)
            | u32::from(pixel[2])
    }));
    conn.change_property32(
        PropMode::REPLACE,
        window,
        icon_atom,
        AtomEnum::CARDINAL,
        &icon_data,
    )?;
    let close = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        protocols,
        AtomEnum::ATOM,
        &[close],
    )?;
    conn.create_gc(gc, window, &CreateGCAux::new())?;
    conn.map_window(window)?;
    conn.flush()?;
    let first = conn.setup().min_keycode;
    let mapping = conn
        .get_keyboard_mapping(first, conn.setup().max_keycode - first + 1)?
        .reply()?;
    eprintln!("WINDOW id={window} pid={}", std::process::id());
    app.navigate(initial, None, true);
    let mut space = Workspace::new(Limits::default())?;
    let opened = space.open_window(app).map_err(|error| error.error)?;
    let atoms = Atoms {
        protocols,
        close,
        theme: theme_atom,
        utf8: utf8_atom,
        icon: icon_atom,
        icon_data,
    };
    let mut desktop = Desktop {
        space,
        root: screen.root,
        native: BTreeMap::from([(
            opened.window,
            NativeWindow {
                xid: window,
                gc,
                width: surface_width,
                height: surface_height,
                scale: applied_scale,
                theme: window_theme,
                dirty: true,
            },
        )]),
        initial_tab: opened.tab,
        strip: false,
        drag: None,
        fonts: platform::load_fonts()?,
        system_theme: ColorScheme::Light,
        system_scale,
        scripts: restart_scripts,
        session,
        notice: None,
        titles: BTreeMap::new(),
    };
    for url in restore_tabs {
        desktop.fresh_tab(opened.pane, Some(url), &bookmarks, &conn)?;
    }
    let mut update_status = if auto_update {
        "Automatic updates enabled"
    } else {
        "Automatic checks off; use Check for updates"
    }
    .to_string();
    let mut update_progress = None;
    let mut last_tabs = Vec::new();
    loop {
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::Expose(e) => {
                    if let Some(id) = desktop.window_id(e.window) {
                        desktop.native.get_mut(&id).unwrap().dirty = true;
                    }
                }
                Event::ConfigureNotify(e) => {
                    if let Some(id) = desktop.window_id(e.window) {
                        let native = desktop.native.get_mut(&id).unwrap();
                        let width = u32::from(e.width).clamp(1, 4800);
                        let height = u32::from(e.height).clamp(1, 3600);
                        if native.width != width || native.height != height {
                            native.width = width;
                            native.height = height;
                            desktop.cancel_drag(&conn)?;
                            desktop.sync_geometry(&conn)?;
                        }
                    }
                }
                Event::ButtonPress(e) => {
                    if let Some(id) = desktop.window_id(e.event) {
                        desktop.press(id, e, &conn, &bookmarks)?;
                    }
                }
                Event::ButtonRelease(e) => {
                    if let Some(id) = desktop.window_id(e.event) {
                        desktop.release(id, e, &conn, &atoms, screen_num)?;
                    }
                }
                Event::MotionNotify(e) => desktop.motion((e.root_x, e.root_y), &conn)?,
                Event::KeyPress(e) => {
                    if let Some(id) = desktop.window_id(e.event) {
                        let shift = e.state.contains(KeyButMask::SHIFT);
                        let ctrl = e.state.contains(KeyButMask::CONTROL);
                        let alt = e.state.contains(KeyButMask::MOD1);
                        let base = e.detail.saturating_sub(first) as usize
                            * mapping.keysyms_per_keycode as usize;
                        let mut sym = *mapping.keysyms.get(base + usize::from(shift)).unwrap_or(&0);
                        if sym == 0 {
                            sym = *mapping.keysyms.get(base).unwrap_or(&0);
                        }
                        if let Some(key) = platform::translate_keysym(sym) {
                            desktop.key(id, key, ctrl, shift, alt, &conn, &bookmarks)?;
                        }
                    }
                }
                Event::ClientMessage(e) if e.data.as_data32()[0] == close => {
                    if let Some(id) = desktop.window_id(e.window) {
                        desktop.close_window(id, false, &conn)?;
                    }
                }
                Event::DestroyNotify(e) => {
                    if let Some(id) = desktop.window_id(e.window) {
                        desktop.close_window(id, true, &conn)?;
                    }
                }
                Event::FocusIn(e) => {
                    if let Some(id) = desktop.window_id(e.event) {
                        desktop.focus_window(id)?;
                    }
                }
                _ => {}
            }
            if desktop.space.is_empty() {
                return Ok(());
            }
        }
        for (_, app) in desktop.space.tabs_mut() {
            app.poll();
        }
        let ids = desktop.space.tabs().map(|(id, _)| id).collect::<Vec<_>>();
        for id in ids {
            let refresh = desktop.space.get_mut(id).unwrap().take_bookmark_refresh();
            let change = desktop.space.get_mut(id).unwrap().take_bookmark_change();
            if refresh || change.is_some() {
                let result = if let Some(change) = &change {
                    bookmarks.apply(change)
                } else {
                    bookmarks.load()
                };
                match result {
                    Ok(entries) => {
                        for (_, app) in desktop.space.tabs_mut() {
                            app.set_bookmarks(entries.clone());
                        }
                        if let Some(change) = change {
                            desktop.space.get_mut(id).unwrap().set_bookmark_status(
                                match change {
                                    mg_chassis::bookmarks::BookmarkChange::Add(_) => {
                                        "Bookmark saved"
                                    }
                                    mg_chassis::bookmarks::BookmarkChange::Remove(_) => {
                                        "Bookmark removed"
                                    }
                                }
                                .into(),
                            );
                        }
                    }
                    Err(error) => desktop
                        .space
                        .get_mut(id)
                        .unwrap()
                        .set_bookmark_status(error),
                }
            }
        }
        while let Ok(theme) = system_themes.try_recv() {
            desktop.system_theme = theme;
            for (_, app) in desktop.space.tabs_mut() {
                app.set_system_theme(theme);
            }
            desktop.dirty_all();
        }
        let mut geometry_changed = false;
        if Instant::now() >= next_scale_check {
            let scale = platform::scaling::read_scale(&conn, screen_num).unwrap_or(100);
            if desktop.system_scale != scale {
                desktop.system_scale = scale;
                for (_, app) in desktop.space.tabs_mut() {
                    app.set_system_scale(scale);
                }
                geometry_changed = true;
            }
            next_scale_check = Instant::now() + Duration::from_secs(2);
        }
        let mut preferences = None;
        for (_, app) in desktop.space.tabs_mut() {
            let theme = app.take_theme_change().is_some();
            let scale = app.take_scale_change().is_some();
            if theme || scale {
                preferences = Some(Preferences {
                    theme: app.theme_preference(),
                    scale: app.scale_preference(),
                });
                geometry_changed |= scale;
            }
        }
        if let Some(preferences) = preferences {
            let status = match settings.save(preferences) {
                Ok(()) => "Settings saved".into(),
                Err(error) => {
                    eprintln!("SETTINGS: {error}");
                    format!("Session only: {error}")
                }
            };
            for (_, app) in desktop.space.tabs_mut() {
                app.set_theme_preference(preferences.theme);
                app.set_scale_preference(preferences.scale);
                app.set_settings_status(status.clone());
            }
            desktop.dirty_all();
        }
        if geometry_changed {
            desktop.cancel_drag(&conn)?;
            desktop.sync_geometry(&conn)?;
        }
        let windows = desktop.native.keys().copied().collect::<Vec<_>>();
        for id in windows {
            let theme = desktop
                .space
                .get(desktop.active_in(id))
                .unwrap()
                .effective_theme();
            let native = desktop.native.get_mut(&id).unwrap();
            if native.theme != Some(theme) {
                conn.change_property8(
                    PropMode::REPLACE,
                    native.xid,
                    atoms.theme,
                    atoms.utf8,
                    if theme == ColorScheme::Dark {
                        b"dark"
                    } else {
                        b""
                    },
                )?;
                native.theme = Some(theme);
                native.dirty = true;
            }
        }
        let requested = desktop.space.tabs_mut().fold(false, |requested, (_, app)| {
            app.take_update_request() || requested
        });
        if !update_running
            && !restart_ready
            && (requested || (auto_update && Instant::now() >= next_update))
        {
            update_running = true;
            next_update = Instant::now() + Duration::from_secs(24 * 60 * 60);
            update_status = "Checking for updates...".into();
            update_progress = None;
            for (_, app) in desktop.space.tabs_mut() {
                app.set_update_status(update_status.clone());
            }
            let tx = update_tx.clone();
            let target = executable.clone();
            thread::spawn(move || {
                let result = updater::update_with_progress(&target, &mut |event| {
                    let _ = tx.send(event);
                });
                let _ = tx.send(updater::UpdateEvent::Finished(result));
            });
        }
        while let Ok(event) = update_rx.try_recv() {
            match event {
                updater::UpdateEvent::Status(status) => {
                    update_status = status.into();
                    update_progress = None;
                    for (_, app) in desktop.space.tabs_mut() {
                        app.set_update_status(update_status.clone());
                    }
                }
                updater::UpdateEvent::Download { received, total } => {
                    update_progress = Some((received, total));
                    for (_, app) in desktop.space.tabs_mut() {
                        app.set_update_progress(received, total);
                    }
                }
                updater::UpdateEvent::Finished(result) => {
                    update_running = false;
                    restart_ready = result
                        .as_ref()
                        .is_ok_and(|message| message.starts_with("Installed "));
                    update_status =
                        result.unwrap_or_else(|error| format!("Update failed: {error}"));
                    update_progress = None;
                    eprintln!("UPDATE: {update_status}");
                    for (_, app) in desktop.space.tabs_mut() {
                        app.set_restart_available(restart_ready);
                        app.set_update_status(update_status.clone());
                    }
                }
            }
        }
        let count = desktop.space.tabs().count();
        if membership_changed(&mut last_tabs, &desktop.space) {
            for (_, app) in desktop.space.tabs_mut() {
                app.set_restart_tab_count(count);
                app.set_restart_available(restart_ready);
                app.set_update_status(update_status.clone());
                if let Some((received, total)) = update_progress {
                    app.set_update_progress(received, total);
                }
                app.set_debugging(cdp.is_some());
            }
        }
        let restart = desktop.space.tabs_mut().fold(false, |requested, (_, app)| {
            app.take_restart_request() || requested
        });
        if restart && restart_ready {
            let urls = desktop
                .space
                .windows()
                .flat_map(|(_, window)| window.panes())
                .flat_map(|pane| pane.tabs())
                .map(|&tab| {
                    let url = desktop.space.get(tab).unwrap().page_url();
                    if url.starts_with("http://") || url.starts_with("https://") {
                        url.to_string()
                    } else {
                        "https://example.com/".into()
                    }
                })
                .collect::<Vec<_>>();
            match crate::restart::launch_tabs(&executable, &urls, restart_scripts, auto_update) {
                Ok(child) => {
                    eprintln!("RESTART: launched pid={}", child.id());
                    return Ok(());
                }
                Err(error) => {
                    for (_, app) in desktop.space.tabs_mut() {
                        app.set_update_status(error.clone());
                    }
                }
            }
        }
        if let Some(protocol) = &mut cdp {
            let initial = desktop.initial_tab;
            let names = desktop
                .space
                .tabs()
                .map(|(id, _)| {
                    if id == initial {
                        "page-1".into()
                    } else {
                        format!("page-{}", id.get())
                    }
                })
                .collect::<Vec<String>>();
            let mut pages = desktop
                .space
                .tabs_mut()
                .zip(names.iter())
                .map(|((_, app), name)| (name.as_str(), app))
                .collect::<Vec<_>>();
            if let Err(error) = protocol.tick_pages(&mut pages) {
                drop(pages);
                cdp = None;
                for (_, app) in desktop.space.tabs_mut() {
                    app.set_debugging(false);
                }
                desktop.notify(format!("Remote debugging stopped: {error}"));
            }
        }
        desktop.paint(&conn, depth)?;
        let journey_result = desktop
            .space
            .tabs()
            .find_map(|(_, app)| app.journey_result());
        if let Some(success) = journey_result {
            if !success {
                drop(desktop);
                std::process::exit(2);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod display_coordinate_tests {
    use super::*;

    #[test]
    fn native_focus_transfer_cancels_old_press_but_same_window_focus_preserves_it() {
        let app = || {
            App::new(
                platform::load_fonts().unwrap(),
                std::sync::Arc::new(mg_chassis::scripts::DisabledScripts),
            )
        };
        let mut space = Workspace::new(Limits::default()).unwrap();
        let first = space.open_window(app()).map_err(|r| r.error).unwrap();
        let second = space.open_window(app()).map_err(|r| r.error).unwrap();
        space.focus_pane(first.pane).unwrap();
        let mut desktop = Desktop {
            space,
            root: 0,
            native: BTreeMap::new(),
            initial_tab: first.tab,
            strip: false,
            drag: None,
            fonts: platform::load_fonts().unwrap(),
            system_theme: ColorScheme::Light,
            system_scale: 100,
            scripts: false,
            session: mg_chassis::net::Session::default(),
            notice: None,
            titles: BTreeMap::new(),
        };
        let first_app = desktop.space.get_mut(first.tab).unwrap();
        let baseline = first_app.paint().pixels;
        let menu_x = first_app.width() as i32 - 40;
        first_app.pointer_down(menu_x, 20);
        desktop.focus_window(second.window).unwrap();
        assert_eq!(desktop.space.focused_window(), Some(second.window));
        let first_app = desktop.space.get_mut(first.tab).unwrap();
        first_app.pointer_up(menu_x, 20);
        assert_eq!(first_app.paint().pixels, baseline);

        desktop.focus_window(first.window).unwrap();
        desktop
            .space
            .get_mut(first.tab)
            .unwrap()
            .pointer_down(menu_x, 20);
        // X11 sends focus notifications during grabs without changing windows.
        desktop.focus_window(first.window).unwrap();
        let first_app = desktop.space.get_mut(first.tab).unwrap();
        first_app.pointer_up(menu_x, 20);
        assert_ne!(
            first_app.paint().pixels,
            baseline,
            "same-window focus must still open the menu"
        );
    }

    #[test]
    fn native_coordinates_and_sizes_follow_the_same_scale() {
        for percent in [75, 100, 125, 150, 175, 200, 250, 300] {
            assert_eq!(scaled_dimension(400, percent), 4 * u32::from(percent));
            assert_eq!(logical_coordinate((4 * percent) as i16, percent), 400);
            assert_eq!(
                logical_coordinate(-1, percent),
                -1 - i32::from(percent == 75)
            );
        }
        assert_eq!(logical_coordinate(501, 200), 250);
        assert_eq!(scaled_dimension(u32::MAX, 300), u32::MAX);
    }

    #[test]
    fn same_count_replacement_receives_shared_host_state_but_moves_do_not_reset_it() {
        let mut space = Workspace::new(Limits::default()).unwrap();
        let first = space.open_window(()).unwrap();
        let second = space.open_tab(first.pane, ()).unwrap();
        let mut previous = Vec::new();
        assert!(membership_changed(&mut previous, &space));
        assert!(!membership_changed(&mut previous, &space));
        space.close_tab(second).unwrap();
        let replacement = space.open_tab(first.pane, ()).unwrap();
        assert_eq!(space.tabs().count(), 2);
        assert!(membership_changed(&mut previous, &space));
        let plan = space.prepare_detach(replacement).unwrap();
        space.commit_detach(plan).unwrap();
        assert!(!membership_changed(&mut previous, &space));
    }
}
