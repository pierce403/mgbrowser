//! Host-owned tabs and windows, without display, persistence or engine dependencies.
//!
//! A payload is owned exactly once for its whole tab lifetime. Moving a tab changes
//! only its placement: it never reloads, clones or drops the payload. The desktop
//! host supplies OS windows, paints visible groups and routes input to their active
//! tabs. `prepare_detach` lets it create a window before committing a state move.
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub fn get(self) -> u64 {
                self.0
            }
        }
    };
}

identity!(TabId);
identity!(WindowId);
identity!(PaneId);

/// Application-wide limits, independent of each tab's engine resource limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub tabs: usize,
    pub windows: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            tabs: 16,
            windows: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidLimits,
    TabLimit,
    WindowLimit,
    PaneLimit,
    UnknownTab,
    UnknownWindow,
    UnknownPane,
    InvalidIndex,
    OnlyTabInWindow,
    StaleDetach,
    IdentityExhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "Tab and window limits must be nonzero",
            Self::TabLimit => "Maximum number of tabs reached",
            Self::WindowLimit => "Maximum number of windows reached",
            Self::PaneLimit => "A window supports at most two side-by-side groups",
            Self::UnknownTab => "Tab no longer exists",
            Self::UnknownWindow => "Window no longer exists",
            Self::UnknownPane => "Pane group no longer exists",
            Self::InvalidIndex => "Tab position is outside the destination group",
            Self::OnlyTabInWindow => "A single tab cannot split into two groups",
            Self::StaleDetach => "Workspace changed while creating the detached window",
            Self::IdentityExhausted => "Workspace identity space exhausted",
        })
    }
}

impl std::error::Error for Error {}

/// Failed creation returns the untouched payload to its caller.
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub payload: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Location {
    pub window: WindowId,
    pub pane: PaneId,
    pub index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Opened {
    pub window: WindowId,
    pub pane: PaneId,
    pub tab: TabId,
}

/// One selected tab from a visible group. Coordinates belong to the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibleTab {
    pub window: WindowId,
    pub pane: PaneId,
    pub tab: TabId,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Pane {
    id: PaneId,
    tabs: Vec<TabId>,
    active: TabId,
}

impl Pane {
    pub fn id(&self) -> PaneId {
        self.id
    }

    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub fn active(&self) -> TabId {
        self.active
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Window {
    // At most two, ordered left to right. Neither windows nor groups are empty.
    panes: Vec<Pane>,
    focused: PaneId,
}

impl Window {
    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn focused_pane(&self) -> PaneId {
        self.focused
    }
}

/// A checked but uncommitted detach. Dropping it is cancellation without mutation.
///
/// The caller creates an OS window for `window()`, then calls `commit_detach`.
/// If that commit fails, it must destroy only that newly created OS window. Any
/// intervening workspace mutation invalidates this plan; payload polling does not.
#[derive(Debug)]
pub struct DetachPlan {
    owner: Arc<()>,
    revision: u64,
    tab: TabId,
    window: WindowId,
    pane: PaneId,
}

impl DetachPlan {
    pub fn tab(&self) -> TabId {
        self.tab
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    pub fn pane(&self) -> PaneId {
        self.pane
    }
}

/// Side effects the host must mirror after a move or close succeeds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Removed {
    pub window: Option<WindowId>,
    pub pane: Option<PaneId>,
}

#[derive(Debug)]
pub struct Closed<T> {
    pub payload: T,
    pub removed: Removed,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Workspace<T> {
    identity: Arc<()>,
    tabs: BTreeMap<TabId, T>,
    windows: BTreeMap<WindowId, Window>,
    focused: Option<WindowId>,
    limits: Limits,
    next_id: u64,
    revision: u64,
}

impl<T> Workspace<T> {
    pub fn new(limits: Limits) -> Result<Self, Error> {
        if limits.tabs == 0 || limits.windows == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(Self {
            identity: Arc::new(()),
            tabs: BTreeMap::new(),
            windows: BTreeMap::new(),
            focused: None,
            limits,
            next_id: 1,
            revision: 0,
        })
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn tabs(&self) -> impl Iterator<Item = (TabId, &T)> {
        self.tabs.iter().map(|(&id, payload)| (id, payload))
    }

    /// Poll all payloads, including hidden tabs, without altering placement.
    pub fn tabs_mut(&mut self) -> impl Iterator<Item = (TabId, &mut T)> {
        self.tabs.iter_mut().map(|(&id, payload)| (id, payload))
    }

    pub fn get(&self, tab: TabId) -> Option<&T> {
        self.tabs.get(&tab)
    }

    pub fn get_mut(&mut self, tab: TabId) -> Option<&mut T> {
        self.tabs.get_mut(&tab)
    }

    pub fn windows(&self) -> impl Iterator<Item = (WindowId, &Window)> {
        self.windows.iter().map(|(&id, window)| (id, window))
    }

    pub fn window(&self, window: WindowId) -> Option<&Window> {
        self.windows.get(&window)
    }

    pub fn pane(&self, pane: PaneId) -> Option<&Pane> {
        self.windows
            .values()
            .flat_map(|window| &window.panes)
            .find(|group| group.id == pane)
    }

    pub fn visible(&self) -> impl Iterator<Item = VisibleTab> + '_ {
        self.windows.iter().flat_map(|(&id, window)| {
            window.panes.iter().map(move |pane| VisibleTab {
                window: id,
                pane: pane.id,
                tab: pane.active,
            })
        })
    }

    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused
    }

    pub fn active_tab(&self) -> Option<TabId> {
        let window = self.windows.get(&self.focused?)?;
        window
            .panes
            .iter()
            .find(|pane| pane.id == window.focused)
            .map(|pane| pane.active)
    }

    pub fn location(&self, tab: TabId) -> Option<Location> {
        self.windows.iter().find_map(|(&window, state)| {
            state.panes.iter().find_map(|pane| {
                pane.tabs
                    .iter()
                    .position(|&id| id == tab)
                    .map(|index| Location {
                        window,
                        pane: pane.id,
                        index,
                    })
            })
        })
    }

    /// Create a window containing its first tab. Failure retains the payload.
    pub fn open_window(&mut self, payload: T) -> Result<Opened, Rejected<T>> {
        let check = self.check_ids(3).and_then(|()| {
            if self.tabs.len() >= self.limits.tabs {
                Err(Error::TabLimit)
            } else if self.windows.len() >= self.limits.windows {
                Err(Error::WindowLimit)
            } else {
                Ok(())
            }
        });
        if let Err(error) = check {
            return Err(Rejected { error, payload });
        }
        let opened = Opened {
            window: WindowId(self.next_id),
            pane: PaneId(self.next_id + 1),
            tab: TabId(self.next_id + 2),
        };
        self.tabs.insert(opened.tab, payload);
        self.insert_window(opened.window, opened.pane, opened.tab);
        self.finish(3);
        Ok(opened)
    }

    /// Append and select a tab in an existing group.
    pub fn open_tab(&mut self, pane: PaneId, payload: T) -> Result<TabId, Rejected<T>> {
        let check = self.check_ids(1).and_then(|()| {
            if self.tabs.len() >= self.limits.tabs {
                Err(Error::TabLimit)
            } else {
                self.pane_window(pane)
            }
        });
        let window = match check {
            Ok(window) => window,
            Err(error) => return Err(Rejected { error, payload }),
        };
        let tab = TabId(self.next_id);
        self.tabs.insert(tab, payload);
        let group = self.pane_mut(window, pane);
        group.tabs.push(tab);
        group.active = tab;
        self.focus(window, pane);
        self.finish(1);
        Ok(tab)
    }

    pub fn select_tab(&mut self, tab: TabId) -> Result<(), Error> {
        self.check_ids(0)?;
        let location = self.location(tab).ok_or(Error::UnknownTab)?;
        self.pane_mut(location.window, location.pane).active = tab;
        self.focus(location.window, location.pane);
        self.finish(0);
        Ok(())
    }

    pub fn focus_pane(&mut self, pane: PaneId) -> Result<(), Error> {
        self.check_ids(0)?;
        let window = self.pane_window(pane)?;
        self.focus(window, pane);
        self.finish(0);
        Ok(())
    }

    pub fn reorder_tab(&mut self, tab: TabId, index: usize) -> Result<(), Error> {
        let pane = self.location(tab).ok_or(Error::UnknownTab)?.pane;
        self.move_tab(tab, pane, index).map(|_| ())
    }

    /// Move and select a tab. `index` is measured after removing it from its old
    /// group, including when reordering within the same group.
    pub fn move_tab(&mut self, tab: TabId, pane: PaneId, index: usize) -> Result<Removed, Error> {
        self.check_ids(0)?;
        let source = self.location(tab).ok_or(Error::UnknownTab)?;
        let window = self.pane_window(pane)?;
        let length = self.pane(pane).unwrap().tabs.len() - usize::from(source.pane == pane);
        if index > length {
            return Err(Error::InvalidIndex);
        }
        let removed = if source.pane == pane {
            self.pane_mut(window, pane).tabs.remove(source.index);
            Removed::default()
        } else {
            self.remove_membership(source, tab)
        };
        let group = self.pane_mut(window, pane);
        group.tabs.insert(index, tab);
        group.active = tab;
        self.focus(window, pane);
        self.finish(0);
        Ok(removed)
    }

    /// Create a new left/right group with this tab. An existing two-group window
    /// must instead receive the tab through `move_tab` into the chosen group.
    pub fn dock_tab(
        &mut self,
        tab: TabId,
        window: WindowId,
        side: Side,
    ) -> Result<(PaneId, Removed), Error> {
        self.check_ids(1)?;
        let source = self.location(tab).ok_or(Error::UnknownTab)?;
        let target = self.windows.get(&window).ok_or(Error::UnknownWindow)?;
        if target.panes.len() >= 2 {
            return Err(Error::PaneLimit);
        }
        if source.window == window && target.panes[0].tabs.len() == 1 {
            return Err(Error::OnlyTabInWindow);
        }
        let pane = PaneId(self.next_id);
        let removed = self.remove_membership(source, tab);
        let target = self.windows.get_mut(&window).unwrap();
        target.panes.insert(
            if side == Side::Left {
                0
            } else {
                target.panes.len()
            },
            Pane {
                id: pane,
                tabs: vec![tab],
                active: tab,
            },
        );
        self.focus(window, pane);
        self.finish(1);
        Ok((pane, removed))
    }

    pub fn prepare_detach(&self, tab: TabId) -> Result<DetachPlan, Error> {
        self.check_ids(2)?;
        let source = self.location(tab).ok_or(Error::UnknownTab)?;
        let source_window = self.windows.get(&source.window).unwrap();
        let replaces_window =
            source_window.panes.len() == 1 && source_window.panes[0].tabs.len() == 1;
        if !replaces_window && self.windows.len() >= self.limits.windows {
            return Err(Error::WindowLimit);
        }
        Ok(DetachPlan {
            owner: self.identity.clone(),
            revision: self.revision,
            tab,
            window: WindowId(self.next_id),
            pane: PaneId(self.next_id + 1),
        })
    }

    pub fn commit_detach(&mut self, plan: DetachPlan) -> Result<(Opened, Removed), Error> {
        if !Arc::ptr_eq(&self.identity, &plan.owner)
            || self.revision != plan.revision
            || self.next_id != plan.window.0
        {
            return Err(Error::StaleDetach);
        }
        // Revalidate before any mutation, including limits and identity capacity.
        self.prepare_detach(plan.tab)?;
        let source = self.location(plan.tab).unwrap();
        let removed = self.remove_membership(source, plan.tab);
        self.insert_window(plan.window, plan.pane, plan.tab);
        self.finish(2);
        Ok((
            Opened {
                window: plan.window,
                pane: plan.pane,
                tab: plan.tab,
            },
            removed,
        ))
    }

    /// Return the closed payload so the caller controls worker cleanup timing.
    pub fn close_tab(&mut self, tab: TabId) -> Result<Closed<T>, Error> {
        self.check_ids(0)?;
        let source = self.location(tab).ok_or(Error::UnknownTab)?;
        let removed = self.remove_membership(source, tab);
        let payload = self.tabs.remove(&tab).unwrap();
        self.finish(0);
        Ok(Closed { payload, removed })
    }

    /// Close only the named window, returning its payloads in visible tab order.
    /// The host exits only when `is_empty()` is true after this operation.
    pub fn close_window(&mut self, window: WindowId) -> Result<Vec<(TabId, T)>, Error> {
        self.check_ids(0)?;
        if !self.windows.contains_key(&window) {
            return Err(Error::UnknownWindow);
        }
        let window_state = self.windows.remove(&window).unwrap();
        let closed = window_state
            .panes
            .into_iter()
            .flat_map(|pane| pane.tabs)
            .map(|tab| (tab, self.tabs.remove(&tab).unwrap()))
            .collect();
        self.repair_focus();
        self.finish(0);
        Ok(closed)
    }

    fn check_ids(&self, new_ids: u64) -> Result<(), Error> {
        self.next_id
            .checked_add(new_ids)
            .ok_or(Error::IdentityExhausted)?;
        self.revision
            .checked_add(1)
            .ok_or(Error::IdentityExhausted)?;
        Ok(())
    }

    fn finish(&mut self, new_ids: u64) {
        self.next_id += new_ids;
        self.revision += 1;
    }

    fn pane_window(&self, pane: PaneId) -> Result<WindowId, Error> {
        self.windows
            .iter()
            .find_map(|(&id, window)| {
                window
                    .panes
                    .iter()
                    .any(|group| group.id == pane)
                    .then_some(id)
            })
            .ok_or(Error::UnknownPane)
    }

    fn pane_mut(&mut self, window: WindowId, pane: PaneId) -> &mut Pane {
        self.windows
            .get_mut(&window)
            .unwrap()
            .panes
            .iter_mut()
            .find(|group| group.id == pane)
            .unwrap()
    }

    fn focus(&mut self, window: WindowId, pane: PaneId) {
        self.focused = Some(window);
        self.windows.get_mut(&window).unwrap().focused = pane;
    }

    fn repair_focus(&mut self) {
        if !self
            .focused
            .is_some_and(|window| self.windows.contains_key(&window))
        {
            self.focused = self.windows.keys().next().copied();
        }
    }

    fn insert_window(&mut self, window: WindowId, pane: PaneId, tab: TabId) {
        self.windows.insert(
            window,
            Window {
                panes: vec![Pane {
                    id: pane,
                    tabs: vec![tab],
                    active: tab,
                }],
                focused: pane,
            },
        );
        self.focused = Some(window);
    }

    fn remove_membership(&mut self, source: Location, tab: TabId) -> Removed {
        let mut removed = Removed::default();
        let window = self.windows.get_mut(&source.window).unwrap();
        let index = window
            .panes
            .iter()
            .position(|pane| pane.id == source.pane)
            .unwrap();
        let group = &mut window.panes[index];
        group.tabs.remove(source.index);
        if group.tabs.is_empty() {
            window.panes.remove(index);
            removed.pane = Some(source.pane);
            if window.panes.is_empty() {
                self.windows.remove(&source.window);
                removed.window = Some(source.window);
            } else if window.focused == source.pane {
                window.focused = window.panes[index.min(window.panes.len() - 1)].id;
            }
        } else if group.active == tab {
            group.active = group.tabs[source.index.min(group.tabs.len() - 1)];
        }
        self.repair_focus();
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    fn workspace<T>() -> Workspace<T> {
        Workspace::new(Limits::default()).unwrap()
    }

    fn invariant<T>(space: &Workspace<T>) {
        let mut seen = std::collections::BTreeSet::new();
        assert!(space.tabs.len() <= space.limits.tabs);
        assert!(space.windows.len() <= space.limits.windows);
        assert_eq!(space.tabs.is_empty(), space.windows.is_empty());
        assert_eq!(space.focused.is_none(), space.windows.is_empty());
        for (&id, window) in &space.windows {
            assert!((1..=2).contains(&window.panes.len()));
            assert!(window.panes.iter().any(|pane| pane.id == window.focused));
            for pane in &window.panes {
                assert!(!pane.tabs.is_empty());
                assert!(pane.tabs.contains(&pane.active));
                for (index, &tab) in pane.tabs.iter().enumerate() {
                    assert!(seen.insert(tab));
                    assert!(space.tabs.contains_key(&tab));
                    assert_eq!(
                        space.location(tab),
                        Some(Location {
                            window: id,
                            pane: pane.id,
                            index
                        })
                    );
                }
            }
        }
        assert_eq!(seen.len(), space.tabs.len());
        if let Some(window) = space.focused {
            assert!(space.windows.contains_key(&window));
        }
    }

    #[test]
    fn tabs_switch_reorder_and_keep_independent_state() {
        let mut space = workspace();
        let first = space
            .open_window(vec!["first history", "typed form"])
            .unwrap();
        let second = space.open_tab(first.pane, vec!["second history"]).unwrap();
        let third = space.open_tab(first.pane, vec!["third history"]).unwrap();
        space.select_tab(first.tab).unwrap();
        space.get_mut(first.tab).unwrap().push("new edit");
        space.reorder_tab(first.tab, 2).unwrap();
        assert_eq!(
            space.pane(first.pane).unwrap().tabs(),
            &[second, third, first.tab]
        );
        assert_eq!(space.active_tab(), Some(first.tab));
        assert_eq!(
            space.get(first.tab).unwrap(),
            &["first history", "typed form", "new edit"]
        );
        assert_eq!(space.get(second).unwrap(), &["second history"]);
        space.reorder_tab(first.tab, 0).unwrap();
        assert_eq!(
            space.pane(first.pane).unwrap().tabs(),
            &[first.tab, second, third]
        );
        invariant(&space);
    }

    #[derive(Debug)]
    struct Payload {
        state: Box<usize>,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    #[test]
    fn detachment_and_docking_move_exact_payload_without_reload_or_drop() {
        let drops = Rc::new(Cell::new(0));
        let mut space = workspace();
        let first = space
            .open_window(Payload {
                state: Box::new(42),
                drops: drops.clone(),
            })
            .unwrap();
        let other = space
            .open_tab(
                first.pane,
                Payload {
                    state: Box::new(9),
                    drops: drops.clone(),
                },
            )
            .unwrap();
        let pointer = space.get(first.tab).unwrap().state.as_ref() as *const usize;
        let plan = space.prepare_detach(first.tab).unwrap();
        assert_eq!(
            space.windows().count(),
            1,
            "Preparing or failed OS creation must not move state"
        );
        assert_eq!(plan.tab(), first.tab);
        let created_window = plan.window();
        let created_pane = plan.pane();
        let (detached, removed) = space.commit_detach(plan).unwrap();
        assert_eq!(detached.window, created_window);
        assert_eq!(detached.pane, created_pane);
        assert_eq!(removed, Removed::default());
        assert_eq!(
            space.get(first.tab).unwrap().state.as_ref() as *const usize,
            pointer
        );
        let (left, removed) = space.dock_tab(first.tab, first.window, Side::Left).unwrap();
        assert_eq!(removed.window, Some(detached.window));
        assert_eq!(space.window(first.window).unwrap().panes()[0].id(), left);
        assert_eq!(
            space.get(first.tab).unwrap().state.as_ref() as *const usize,
            pointer
        );
        let removed = space.move_tab(first.tab, first.pane, 1).unwrap();
        assert_eq!(removed.pane, Some(left));
        assert_eq!(removed.window, None);
        assert_eq!(space.pane(first.pane).unwrap().tabs(), &[other, first.tab]);
        assert_eq!(drops.get(), 0);
        invariant(&space);
        let closed = space.close_tab(first.tab).unwrap();
        assert_eq!(*closed.payload.state, 42);
        assert_eq!(drops.get(), 0, "Caller owns cleanup of a closed payload");
        drop(closed);
        assert_eq!(drops.get(), 1);
        drop(space);
        assert_eq!(drops.get(), 2);
    }

    #[test]
    fn invalid_operations_leave_every_field_unchanged() {
        let mut space = workspace();
        let first = space.open_window("one").unwrap();
        let before = format!("{space:?}");
        assert_eq!(space.reorder_tab(first.tab, 1), Err(Error::InvalidIndex));
        assert_eq!(
            space.move_tab(first.tab, PaneId(99), 0),
            Err(Error::UnknownPane)
        );
        assert_eq!(
            space.move_tab(TabId(99), first.pane, 0),
            Err(Error::UnknownTab)
        );
        assert_eq!(
            space.dock_tab(first.tab, WindowId(99), Side::Left),
            Err(Error::UnknownWindow)
        );
        assert_eq!(
            space.dock_tab(first.tab, first.window, Side::Right),
            Err(Error::OnlyTabInWindow)
        );
        assert!(matches!(space.close_tab(TabId(99)), Err(Error::UnknownTab)));
        assert_eq!(space.close_window(WindowId(99)), Err(Error::UnknownWindow));
        assert_eq!(space.select_tab(TabId(99)), Err(Error::UnknownTab));
        assert_eq!(space.focus_pane(PaneId(99)), Err(Error::UnknownPane));
        let rejected = space.open_tab(PaneId(99), "retained").unwrap_err();
        assert_eq!(
            (rejected.error, rejected.payload),
            (Error::UnknownPane, "retained")
        );
        assert_eq!(format!("{space:?}"), before);
        invariant(&space);
    }

    #[test]
    fn cancelled_and_stale_detach_are_transactional() {
        let mut space = workspace();
        let first = space.open_window(1).unwrap();
        let second = space.open_tab(first.pane, 2).unwrap();
        let before = format!("{space:?}");
        drop(space.prepare_detach(first.tab).unwrap());
        assert_eq!(format!("{space:?}"), before);
        let plan = space.prepare_detach(first.tab).unwrap();
        space.select_tab(second).unwrap();
        let before = format!("{space:?}");
        assert_eq!(space.commit_detach(plan), Err(Error::StaleDetach));
        assert_eq!(format!("{space:?}"), before);
        let plan = space.prepare_detach(first.tab).unwrap();
        *space.get_mut(first.tab).unwrap() = 7;
        for (_, payload) in space.tabs_mut() {
            *payload += 1;
        }
        space.commit_detach(plan).unwrap();
        assert_eq!(
            space.get(first.tab),
            Some(&8),
            "Polling payloads does not stale placement"
        );
        invariant(&space);
    }

    #[test]
    fn detach_plans_cannot_cross_workspaces_with_matching_ids() {
        let mut first = workspace();
        let original = first.open_window("first").unwrap();
        let mut second = workspace();
        let other = second.open_window("second").unwrap();
        assert_eq!(original, other);
        let before = format!("{second:?}");
        assert_eq!(
            second.commit_detach(first.prepare_detach(original.tab).unwrap()),
            Err(Error::StaleDetach)
        );
        assert_eq!(format!("{second:?}"), before);
        invariant(&first);
        invariant(&second);
    }

    #[test]
    fn side_by_side_groups_keep_independent_selection_and_focus() {
        let mut space = workspace();
        let first = space.open_window("a").unwrap();
        let b = space.open_tab(first.pane, "b").unwrap();
        let c = space.open_tab(first.pane, "c").unwrap();
        let (right, _) = space.dock_tab(c, first.window, Side::Right).unwrap();
        let d = space.open_tab(right, "d").unwrap();
        assert_eq!(space.pane(first.pane).unwrap().active(), b);
        assert_eq!(space.pane(right).unwrap().active(), d);
        space.select_tab(first.tab).unwrap();
        assert_eq!(space.pane(right).unwrap().active(), d);
        space.focus_pane(right).unwrap();
        assert_eq!(space.active_tab(), Some(d));
        assert_eq!(
            space.visible().map(|view| view.tab).collect::<Vec<_>>(),
            vec![first.tab, d]
        );
        let before = format!("{space:?}");
        assert_eq!(
            space.dock_tab(b, first.window, Side::Left),
            Err(Error::PaneLimit)
        );
        assert_eq!(format!("{space:?}"), before);
        space.close_tab(d).unwrap();
        assert_eq!(space.active_tab(), Some(c));
        space.close_tab(c).unwrap();
        assert_eq!(space.active_tab(), Some(first.tab));
        assert_eq!(space.window(first.window).unwrap().panes().len(), 1);
        invariant(&space);
    }

    #[test]
    fn moving_the_last_tab_closes_only_its_source_window() {
        let mut space = workspace();
        let first = space.open_window("a").unwrap();
        let second = space.open_window("b").unwrap();
        let removed = space.move_tab(first.tab, second.pane, 0).unwrap();
        assert_eq!(
            removed,
            Removed {
                window: Some(first.window),
                pane: Some(first.pane)
            }
        );
        assert!(space.window(first.window).is_none());
        assert_eq!(space.focused_window(), Some(second.window));
        assert_eq!(space.tabs().count(), 2);
        assert!(!space.is_empty());
        invariant(&space);
        let closed = space.close_tab(first.tab).unwrap();
        assert_eq!(closed.payload, "a");
        assert_eq!(closed.removed, Removed::default());
        assert_eq!(space.active_tab(), Some(second.tab));
        assert_eq!(
            space.close_window(second.window).unwrap(),
            vec![(second.tab, "b")]
        );
        assert!(space.is_empty());
        assert_eq!(space.active_tab(), None);
        invariant(&space);
    }

    #[test]
    fn closing_one_window_retains_other_windows_and_tab_order() {
        let mut space = workspace();
        let first = space.open_window("a").unwrap();
        let b = space.open_tab(first.pane, "b").unwrap();
        let (right, _) = space.dock_tab(b, first.window, Side::Right).unwrap();
        let c = space.open_tab(right, "c").unwrap();
        let other = space.open_window("other").unwrap();
        space.focus_pane(right).unwrap();
        assert_eq!(
            space.close_window(first.window).unwrap(),
            vec![(first.tab, "a"), (b, "b"), (c, "c")]
        );
        assert_eq!(space.active_tab(), Some(other.tab));
        assert_eq!(
            space.tabs().collect::<Vec<_>>(),
            vec![(other.tab, &"other")]
        );
        invariant(&space);
        let closed = space.close_tab(other.tab).unwrap();
        assert_eq!(closed.removed.window, Some(other.window));
        assert!(space.is_empty());
        invariant(&space);
    }

    #[test]
    fn limits_return_payloads_and_allow_net_zero_window_replacement() {
        assert_eq!(
            Workspace::<()>::new(Limits {
                tabs: 0,
                windows: 1
            })
            .unwrap_err(),
            Error::InvalidLimits
        );
        assert_eq!(
            Workspace::<()>::new(Limits {
                tabs: 1,
                windows: 0
            })
            .unwrap_err(),
            Error::InvalidLimits
        );
        let mut space = Workspace::new(Limits {
            tabs: 2,
            windows: 1,
        })
        .unwrap();
        let first = space.open_window("a").unwrap();
        let before = format!("{space:?}");
        let rejected = space.open_window("b").unwrap_err();
        assert_eq!(
            (rejected.error, rejected.payload),
            (Error::WindowLimit, "b")
        );
        assert_eq!(format!("{space:?}"), before);
        let (replacement, removed) = space
            .commit_detach(space.prepare_detach(first.tab).unwrap())
            .unwrap();
        assert_eq!(removed.window, Some(first.window));
        assert_ne!(replacement.window, first.window);
        let b = space.open_tab(replacement.pane, "b").unwrap();
        let before = format!("{space:?}");
        assert!(matches!(space.prepare_detach(b), Err(Error::WindowLimit)));
        let rejected = space.open_tab(replacement.pane, "c").unwrap_err();
        assert_eq!((rejected.error, rejected.payload), (Error::TabLimit, "c"));
        assert_eq!(format!("{space:?}"), before);
        invariant(&space);
    }

    #[test]
    fn identities_are_never_reused_and_exhaustion_does_not_wrap() {
        let mut space = workspace();
        let first = space.open_window(1).unwrap();
        space.close_window(first.window).unwrap();
        let second = space.open_window(2).unwrap();
        assert_ne!(first.tab, second.tab);
        assert_ne!(first.window, second.window);
        assert_ne!(first.pane, second.pane);
        space.next_id = u64::MAX;
        let before = format!("{space:?}");
        assert_eq!(
            space.open_tab(second.pane, 3).unwrap_err().error,
            Error::IdentityExhausted
        );
        assert!(matches!(
            space.prepare_detach(second.tab),
            Err(Error::IdentityExhausted)
        ));
        assert_eq!(format!("{space:?}"), before);
        space.revision = u64::MAX;
        let before = format!("{space:?}");
        assert_eq!(space.select_tab(second.tab), Err(Error::IdentityExhausted));
        assert_eq!(format!("{space:?}"), before);
    }

    #[test]
    fn mixed_operations_preserve_ownership_and_rejected_moves_are_atomic() {
        let mut space = Workspace::new(Limits {
            tabs: 8,
            windows: 3,
        })
        .unwrap();
        let mut random = 123_u64;
        for step in 0..2_000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let tabs = space.tabs().map(|(id, _)| id).collect::<Vec<_>>();
            let windows = space.windows().map(|(id, _)| id).collect::<Vec<_>>();
            let panes = space.visible().map(|view| view.pane).collect::<Vec<_>>();
            let pick = |length: usize| (random >> 16) as usize % length;
            let before = format!("{space:?}");
            let result = if tabs.is_empty() {
                space.open_window(step).map(|_| ()).map_err(|e| e.error)
            } else {
                let tab = tabs[pick(tabs.len())];
                let pane = panes[pick(panes.len())];
                let window = windows[pick(windows.len())];
                match random % 10 {
                    0 => space.open_window(step).map(|_| ()).map_err(|e| e.error),
                    1 => space.open_tab(pane, step).map(|_| ()).map_err(|e| e.error),
                    2 => space.select_tab(tab),
                    3 => space.reorder_tab(tab, pick(10)),
                    4 => space.move_tab(tab, pane, pick(10)).map(|_| ()),
                    5 => space.dock_tab(tab, window, Side::Left).map(|_| ()),
                    6 => space.dock_tab(tab, window, Side::Right).map(|_| ()),
                    7 => space
                        .prepare_detach(tab)
                        .and_then(|plan| space.commit_detach(plan))
                        .map(|_| ()),
                    8 => space.close_tab(tab).map(|_| ()),
                    _ => space.close_window(window).map(|_| ()),
                }
            };
            if result.is_err() {
                assert_eq!(format!("{space:?}"), before, "Rejected operation {step}");
            }
            invariant(&space);
        }
    }
}
