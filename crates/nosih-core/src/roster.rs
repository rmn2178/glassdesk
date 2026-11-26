//! Which apps appear in the dock and in what order:
//! 1. pinned apps, some of them gathered in groups;
//! 2. unpinned open apps, initially in opening order;
//! 3. recently closed unpinned apps, as in macOS.

pub const MAX_RECENT: usize = 3;

/// Ids of groups start with this, so they never collide with an app's.
pub const GROUP_PREFIX: &str = "nosih:group:";

#[must_use]
pub fn is_group_id(id: &str) -> bool {
    id.starts_with(GROUP_PREFIX)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockApp {
    pub id: String,
    pub running: bool,
}

/// Pinned apps gathered in one dock icon. The group's id stands in the
/// pinned list, in the group's place; its apps count as pinned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub apps: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
    pinned: Vec<String>,
    groups: Vec<Group>,
    extra: Vec<String>,
    running: Vec<String>,
    recent: Vec<String>,
}

impl Roster {
    #[must_use]
    pub fn new(pinned: Vec<String>, recent: Vec<String>) -> Self {
        let mut roster = Self {
            pinned,
            ..Self::default()
        };
        for id in recent.into_iter().rev() {
            roster.remember(id);
        }
        roster
    }

    /// Adds the groups saved with the pinned list, mending what a
    /// hand-edited file could break: a group missing from the pinned list
    /// gives its apps back to the dock, an app is in one place only, and a
    /// group with fewer than two apps gives way to its app.
    #[must_use]
    pub fn with_groups(mut self, groups: Vec<Group>) -> Self {
        for group in groups {
            let mut apps: Vec<String> = Vec::new();
            for app in group.apps {
                let taken = is_group_id(&app)
                    || apps.contains(&app)
                    || self.pinned.contains(&app)
                    || self.groups.iter().any(|other| other.apps.contains(&app));
                if !taken {
                    apps.push(app);
                }
            }
            let placed = is_group_id(&group.id)
                && self.pinned.contains(&group.id)
                && !self.is_group(&group.id);
            if placed {
                self.groups.push(Group { id: group.id, apps });
            } else {
                self.pinned.extend(apps);
            }
        }
        let groups = &self.groups;
        let mut seen: Vec<String> = Vec::new();
        self.pinned.retain(|id| {
            if !is_group_id(id) {
                return true;
            }
            let keep = groups.iter().any(|group| group.id == *id) && !seen.contains(id);
            seen.push(id.clone());
            keep
        });
        self.tidy_groups();
        // Recovery can move members back to the top level. They remain
        // pinned even though their old group no longer exists.
        let pinned = self.pinned_apps();
        self.recent.retain(|id| !pinned.contains(id));
        self
    }

    /// Updates open apps and returns `true` if the dock's displayed items change.
    pub fn sync<'a>(&mut self, running: impl IntoIterator<Item = &'a str>) -> bool {
        let before = (self.apps(), self.recent.clone());

        let mut now: Vec<String> = Vec::new();
        for id in running {
            if !now.iter().any(|seen| seen == id) {
                now.push(id.to_owned());
            }
        }
        let closed: Vec<String> = self
            .extra
            .iter()
            .filter(|id| !now.contains(id))
            .cloned()
            .collect();
        self.extra.retain(|id| now.contains(id));
        for id in &now {
            // Apps in a group are pinned: they stay in their group.
            if !self.is_pinned(id) && !self.extra.contains(id) {
                self.extra.push(id.clone());
            }
        }
        // Reopened apps are no longer recent; closed apps become recent.
        self.recent.retain(|id| !now.contains(id));
        for id in closed {
            self.remember(id);
        }
        self.running = now;

        (self.apps(), self.recent.clone()) != before
    }

    #[must_use]
    pub fn is_running(&self, id: &str) -> bool {
        self.running.iter().any(|running| running == id)
    }

    /// `true` for a pinned app, on its own or in a group, and for a group.
    #[must_use]
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned.iter().any(|pinned| pinned == id) || self.group_of(id).is_some()
    }

    #[must_use]
    pub fn is_group(&self, id: &str) -> bool {
        self.groups.iter().any(|group| group.id == id)
    }

    #[must_use]
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// The group `app` is in, if any.
    #[must_use]
    pub fn group_of(&self, app: &str) -> Option<&Group> {
        self.groups
            .iter()
            .find(|group| group.apps.iter().any(|member| member == app))
    }

    #[must_use]
    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// Pins an app at the end of the pinned list. If it was open and unpinned,
    /// it keeps its screen position as it moves from the open list to the pinned list.
    /// An app already in a group stays there.
    pub fn pin(&mut self, id: &str) {
        if self.is_pinned(id) {
            return;
        }
        self.extra.retain(|extra| extra != id);
        self.recent.retain(|recent| recent != id);
        self.pinned.push(id.to_owned());
    }

    /// Pins `id` (or moves it, if already pinned) right before `before`, or at
    /// the end of the pinned list when `before` is `None` or not pinned. `id`
    /// may be a group; an app in a group leaves it.
    pub fn pin_before(&mut self, id: &str, before: Option<&str>) {
        if before == Some(id) {
            return;
        }
        self.pinned.retain(|pinned| pinned != id);
        self.extra.retain(|extra| extra != id);
        self.recent.retain(|recent| recent != id);
        self.leave_group(id);
        let at = before
            .and_then(|before| self.pinned.iter().position(|pinned| pinned == before))
            .unwrap_or(self.pinned.len());
        self.pinned.insert(at, id.to_owned());
        self.tidy_groups();
    }

    /// Moves an unpinned open app before another unpinned app, or to the end.
    /// Returns whether the displayed order changed.
    pub fn move_running_before(&mut self, id: &str, before: Option<&str>) -> bool {
        let Some(from) = self.extra.iter().position(|app| app == id) else {
            return false;
        };
        let to = match before {
            Some(before) => {
                let Some(index) = self.extra.iter().position(|app| app == before) else {
                    return false;
                };
                index
            }
            None => self.extra.len(),
        };
        let to = to - usize::from(from < to);
        if from == to {
            return false;
        }
        let app = self.extra.remove(from);
        self.extra.insert(to, app);
        true
    }

    /// Unpins an app. If it is open, it remains in the dock as the first unpinned
    /// app until it closes. An app in a group leaves it; a whole group leaves
    /// the dock with its apps.
    pub fn unpin(&mut self, id: &str) {
        if let Some(group) = self.group(id).cloned() {
            self.pinned.retain(|pinned| pinned != id);
            self.groups.retain(|other| other.id != id);
            for app in group.apps.iter().rev() {
                if self.is_running(app) && !self.extra.contains(app) {
                    self.extra.insert(0, app.clone());
                }
            }
            return;
        }
        if !self.is_pinned(id) {
            return;
        }
        self.pinned.retain(|pinned| pinned != id);
        self.leave_group(id);
        self.tidy_groups();
        if self.is_running(id) {
            self.extra.insert(0, id.to_owned());
        }
    }

    /// Puts `app` in a group with `target`, a pinned app or group in the
    /// dock: into `target` if it is a group, or else into a new group that
    /// takes `target`'s place, `target` first. Returns the group, or `None`
    /// (and changes nothing) if they cannot be grouped.
    pub fn group_with(&mut self, app: &str, target: &str) -> Option<String> {
        if app == target || self.is_group(app) || !self.pinned.iter().any(|pinned| pinned == target)
        {
            return None;
        }
        self.pinned.retain(|pinned| pinned != app);
        self.extra.retain(|extra| extra != app);
        self.recent.retain(|recent| recent != app);
        self.leave_group(app);
        let id = if self.is_group(target) {
            target.to_owned()
        } else {
            let id = self.new_group_id();
            if let Some(at) = self.pinned.iter().position(|pinned| pinned == target) {
                self.pinned[at] = id.clone();
            }
            self.groups.push(Group {
                id: id.clone(),
                apps: vec![target.to_owned()],
            });
            id
        };
        if let Some(group) = self.groups.iter_mut().find(|group| group.id == id) {
            group.apps.push(app.to_owned());
        }
        self.tidy_groups();
        Some(id)
    }

    /// Takes `app` out of its group and pins it right after the group.
    /// Returns `false` if it was in no group.
    pub fn ungroup(&mut self, app: &str) -> bool {
        let Some(group) = self.group_of(app).map(|group| group.id.clone()) else {
            return false;
        };
        self.leave_group(app);
        let at = self
            .pinned
            .iter()
            .position(|pinned| *pinned == group)
            .map_or(self.pinned.len(), |at| at + 1);
        self.pinned.insert(at, app.to_owned());
        self.tidy_groups();
        true
    }

    /// Puts a group's apps back in the dock, in its place and in its order.
    pub fn dissolve(&mut self, id: &str) {
        let Some(index) = self.groups.iter().position(|group| group.id == id) else {
            return;
        };
        let group = self.groups.remove(index);
        match self.pinned.iter().position(|pinned| *pinned == group.id) {
            Some(at) => {
                self.pinned.remove(at);
                for (offset, app) in group.apps.into_iter().enumerate() {
                    self.pinned.insert(at + offset, app);
                }
            }
            None => self.pinned.extend(group.apps),
        }
    }

    /// Top-level pinned items in display order: apps and groups.
    #[must_use]
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    /// Every pinned app, on its own or in a group, in display order.
    #[must_use]
    pub fn pinned_apps(&self) -> Vec<String> {
        let mut apps = Vec::new();
        for id in &self.pinned {
            match self.group(id) {
                Some(group) => apps.extend(group.apps.iter().cloned()),
                None => apps.push(id.clone()),
            }
        }
        apps
    }

    /// Recent apps in display order, newest first.
    #[must_use]
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Takes `id` out of the recent apps (the user dismissed it, or it was
    /// uninstalled). Opening and closing it again makes it recent again.
    /// Returns whether it was there.
    pub fn forget_recent(&mut self, id: &str) -> bool {
        let before = self.recent.len();
        self.recent.retain(|recent| recent != id);
        self.recent.len() != before
    }

    /// Pinned items and open apps in display order, with pinned items first.
    /// A group is running when any of its apps is.
    #[must_use]
    pub fn apps(&self) -> Vec<DockApp> {
        self.pinned
            .iter()
            .chain(&self.extra)
            .map(|id| DockApp {
                id: id.clone(),
                running: match self.group(id) {
                    Some(group) => group.apps.iter().any(|app| self.is_running(app)),
                    None => self.is_running(id),
                },
            })
            .collect()
    }

    /// Removes `app` from any group, leaving the group where it is (see
    /// `tidy_groups`).
    fn leave_group(&mut self, app: &str) {
        for group in &mut self.groups {
            group.apps.retain(|member| member != app);
        }
    }

    /// A group left with one app gives its place to that app; an empty one
    /// disappears.
    fn tidy_groups(&mut self) {
        let mut index = 0;
        while index < self.groups.len() {
            if self.groups[index].apps.len() > 1 {
                index += 1;
                continue;
            }
            let group = self.groups.remove(index);
            let at = self.pinned.iter().position(|pinned| *pinned == group.id);
            let last = group
                .apps
                .into_iter()
                .next()
                .filter(|app| !self.pinned.contains(app));
            match (at, last) {
                (Some(at), Some(app)) => self.pinned[at] = app,
                (Some(at), None) => {
                    self.pinned.remove(at);
                }
                (None, Some(app)) => self.pinned.push(app),
                (None, None) => {}
            }
        }
    }

    fn new_group_id(&self) -> String {
        (1u32..)
            .map(|number| format!("{GROUP_PREFIX}{number}"))
            .find(|id| !self.is_group(id) && !self.pinned.contains(id))
            .unwrap_or_default()
    }

    fn remember(&mut self, id: String) {
        if self.is_pinned(&id) {
            return;
        }
        self.recent.retain(|recent| *recent != id);
        self.recent.insert(0, id);
        self.recent.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::new(vec!["explorer".into(), "browser".into()], Vec::new())
    }

    fn ids(roster: &Roster) -> Vec<String> {
        roster.apps().into_iter().map(|app| app.id).collect()
    }

    #[test]
    fn roster_should_show_pinned_apps_when_nothing_is_running() {
        assert_eq!(ids(&roster()), ["explorer", "browser"]);
    }

    #[test]
    fn roster_should_append_unpinned_running_apps_after_pinned() {
        let mut roster = roster();
        roster.sync(["editor", "browser"]);
        assert_eq!(ids(&roster), ["explorer", "browser", "editor"]);
    }

    #[test]
    fn roster_should_mark_only_running_apps() {
        let mut roster = roster();
        roster.sync(["browser"]);
        let running: Vec<bool> = roster.apps().iter().map(|app| app.running).collect();
        assert_eq!(running, [false, true]);
    }

    #[test]
    fn roster_should_keep_first_seen_order_for_unpinned_apps() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.sync(["terminal", "editor"]);
        assert_eq!(ids(&roster), ["explorer", "browser", "editor", "terminal"]);
    }

    #[test]
    fn dragging_an_open_app_should_keep_its_new_order_after_window_sync() {
        let mut roster = Roster::new(vec!["explorer".into()], Vec::new());
        roster.sync(["editor", "browser", "terminal"]);
        assert!(roster.move_running_before("terminal", Some("editor")));
        roster.sync(["editor", "browser", "terminal"]);
        assert_eq!(ids(&roster), ["explorer", "terminal", "editor", "browser"]);
        assert_eq!(roster.pinned(), ["explorer"]);
    }

    #[test]
    fn dragging_an_open_app_to_the_end_should_keep_it_unpinned() {
        let mut roster = roster();
        roster.sync(["editor", "terminal"]);
        assert!(roster.move_running_before("editor", None));
        assert_eq!(ids(&roster), ["explorer", "browser", "terminal", "editor"]);
        assert!(!roster.move_running_before("editor", None));
    }

    #[test]
    fn roster_should_drop_unpinned_app_when_it_closes() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.sync([]);
        assert_eq!(ids(&roster), ["explorer", "browser"]);
    }

    #[test]
    fn sync_should_report_change_when_an_indicator_changes() {
        assert!(roster().sync(["browser"]));
    }

    #[test]
    fn sync_should_report_no_change_when_only_window_count_changes() {
        let mut roster = roster();
        roster.sync(["browser"]);
        assert!(!roster.sync(["browser", "browser"]));
    }

    #[test]
    fn closing_an_unpinned_app_should_make_it_recent() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.sync([]);
        assert_eq!(roster.recent(), ["editor"]);
    }

    #[test]
    fn closing_a_pinned_app_should_not_make_it_recent() {
        let mut roster = roster();
        roster.sync(["browser"]);
        roster.sync([]);
        assert!(roster.recent().is_empty());
    }

    #[test]
    fn recent_should_keep_only_the_last_three_most_recent_first() {
        let mut roster = roster();
        for app in ["a", "b", "c", "d"] {
            roster.sync([app]);
            roster.sync([]);
        }
        assert_eq!(roster.recent(), ["d", "c", "b"]);
    }

    #[test]
    fn reopening_a_recent_app_should_remove_it_from_recent() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.sync([]);
        roster.sync(["editor"]);
        assert!(roster.recent().is_empty());
    }

    #[test]
    fn forgetting_a_recent_app_removes_only_it_until_it_closes_again() {
        let mut roster = roster();
        for app in ["a", "b"] {
            roster.sync([app]);
            roster.sync([]);
        }
        assert!(roster.forget_recent("a"));
        assert!(!roster.forget_recent("a"));
        assert_eq!(roster.recent(), ["b"]);
        roster.sync(["a"]);
        roster.sync([]);
        assert_eq!(roster.recent(), ["a", "b"]);
    }

    #[test]
    fn sync_should_report_change_when_recent_changes() {
        let mut roster = roster();
        roster.sync(["editor"]);
        assert!(roster.sync([]));
    }

    #[test]
    fn new_should_drop_recent_apps_that_are_pinned() {
        let roster = Roster::new(vec!["browser".into()], vec!["browser".into(), "x".into()]);
        assert_eq!(roster.recent(), ["x"]);
    }

    #[test]
    fn pin_should_keep_a_running_app_in_the_same_place_on_screen() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.pin("editor");
        assert_eq!(ids(&roster), ["explorer", "browser", "editor"]);
    }

    #[test]
    fn pin_should_remove_the_app_from_recent() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.sync([]);
        roster.pin("editor");
        assert!(roster.recent().is_empty());
    }

    #[test]
    fn unpin_should_keep_a_running_app_as_first_unpinned() {
        let mut roster = roster();
        roster.sync(["browser", "editor"]);
        roster.unpin("browser");
        assert_eq!(ids(&roster), ["explorer", "browser", "editor"]);
    }

    #[test]
    fn unpin_should_remove_a_closed_app_from_the_dock() {
        let mut roster = roster();
        roster.unpin("browser");
        assert_eq!(ids(&roster), ["explorer"]);
    }

    #[test]
    fn pin_before_should_move_a_pinned_app_left() {
        let mut roster = Roster::new(vec!["a".into(), "b".into(), "c".into()], Vec::new());
        roster.pin_before("c", Some("b"));
        assert_eq!(roster.pinned(), ["a", "c", "b"]);
    }

    #[test]
    fn pin_before_should_move_a_pinned_app_to_the_end() {
        let mut roster = Roster::new(vec!["a".into(), "b".into(), "c".into()], Vec::new());
        roster.pin_before("a", None);
        assert_eq!(roster.pinned(), ["b", "c", "a"]);
    }

    #[test]
    fn pin_before_should_pin_a_running_app_at_the_given_place() {
        let mut roster = roster();
        roster.sync(["editor"]);
        roster.pin_before("editor", Some("browser"));
        assert_eq!(ids(&roster), ["explorer", "editor", "browser"]);
    }

    #[test]
    fn pin_before_itself_should_change_nothing() {
        let mut roster = roster();
        roster.pin_before("browser", Some("browser"));
        assert_eq!(roster.pinned(), ["explorer", "browser"]);
    }

    #[test]
    fn unpinned_running_app_should_become_recent_when_it_closes() {
        let mut roster = roster();
        roster.sync(["browser"]);
        roster.unpin("browser");
        roster.sync([]);
        assert_eq!(roster.recent(), ["browser"]);
    }

    /// `a`, `b`, `c` pinned, then `c` dropped on `a`.
    fn grouped() -> (Roster, String) {
        let mut roster = Roster::new(vec!["a".into(), "b".into(), "c".into()], Vec::new());
        let group = roster.group_with("c", "a").unwrap_or_default();
        (roster, group)
    }

    fn members(roster: &Roster, group: &str) -> Vec<String> {
        roster
            .group(group)
            .map(|group| group.apps.clone())
            .unwrap_or_default()
    }

    #[test]
    fn group_with_should_put_both_apps_in_one_place_where_the_target_was() {
        let (roster, group) = grouped();
        assert!(is_group_id(&group));
        assert_eq!(roster.pinned(), [group.as_str(), "b"]);
        assert_eq!(members(&roster, &group), ["a", "c"]);
    }

    #[test]
    fn group_with_should_add_to_an_existing_group() {
        let (mut roster, group) = grouped();
        assert_eq!(roster.group_with("b", &group), Some(group.clone()));
        assert_eq!(roster.pinned(), [group.as_str()]);
        assert_eq!(members(&roster, &group), ["a", "c", "b"]);
    }

    #[test]
    fn group_with_should_pin_a_running_app_into_the_group() {
        let (mut roster, group) = grouped();
        roster.sync(["editor"]);
        roster.group_with("editor", &group);
        assert_eq!(ids(&roster), [group.as_str(), "b"]);
        assert!(roster.is_pinned("editor"));
    }

    #[test]
    fn group_with_should_refuse_a_group_itself_or_an_unpinned_target() {
        let (mut roster, group) = grouped();
        roster.sync(["editor"]);
        let before = roster.clone();
        assert_eq!(roster.group_with(&group, "b"), None);
        assert_eq!(roster.group_with("b", "b"), None);
        assert_eq!(roster.group_with("b", "editor"), None);
        assert_eq!(roster, before);
    }

    #[test]
    fn grouped_apps_should_stay_in_their_group_while_running() {
        let (mut roster, group) = grouped();
        roster.sync(["c"]);
        assert_eq!(ids(&roster), [group.as_str(), "b"]);
        assert!(roster.apps()[0].running);
        roster.sync([]);
        assert!(roster.recent().is_empty());
        assert!(!roster.apps()[0].running);
    }

    #[test]
    fn pin_should_not_duplicate_an_app_in_a_group() {
        let (mut roster, group) = grouped();
        roster.pin("c");
        assert_eq!(roster.pinned(), [group.as_str(), "b"]);
    }

    #[test]
    fn ungroup_should_put_the_app_right_after_the_group() {
        let (mut roster, group) = grouped();
        roster.group_with("b", &group);
        assert!(roster.ungroup("c"));
        assert_eq!(roster.pinned(), [group.as_str(), "c"]);
        assert_eq!(members(&roster, &group), ["a", "b"]);
    }

    #[test]
    fn a_group_left_with_one_app_should_give_its_place_to_it() {
        let (mut roster, _) = grouped();
        assert!(roster.ungroup("a"));
        assert_eq!(roster.pinned(), ["c", "a", "b"]);
        assert!(roster.groups().is_empty());
        assert!(!roster.ungroup("a"));
    }

    #[test]
    fn dissolve_should_put_the_apps_back_in_the_group_place() {
        let (mut roster, group) = grouped();
        roster.pin("d");
        roster.dissolve(&group);
        assert_eq!(roster.pinned(), ["a", "c", "b", "d"]);
        assert!(roster.groups().is_empty());
    }

    #[test]
    fn unpin_should_take_an_app_out_of_its_group() {
        let (mut roster, group) = grouped();
        roster.group_with("b", &group);
        roster.sync(["c"]);
        roster.unpin("c");
        assert_eq!(members(&roster, &group), ["a", "b"]);
        assert_eq!(ids(&roster), [group.as_str(), "c"]);
    }

    #[test]
    fn pin_before_should_move_a_group_and_take_an_app_out_of_one() {
        let (mut roster, group) = grouped();
        roster.pin_before(&group, None);
        assert_eq!(roster.pinned(), ["b", group.as_str()]);
        roster.pin_before("c", Some("b"));
        assert_eq!(roster.pinned(), ["c", "b", "a"]);
    }

    #[test]
    fn pinned_apps_should_list_the_apps_inside_groups() {
        let (roster, _) = grouped();
        assert_eq!(roster.pinned_apps(), ["a", "c", "b"]);
    }

    #[test]
    fn with_groups_should_mend_what_a_hand_edited_file_breaks() {
        let pinned = vec![
            "a".to_owned(),
            "nosih:group:1".to_owned(),
            "nosih:group:9".to_owned(),
            "nosih:group:2".to_owned(),
        ];
        let roster = Roster::new(pinned, vec!["x".into()]).with_groups(vec![
            // `a` is pinned already; `x` was recent.
            Group {
                id: "nosih:group:1".into(),
                apps: vec!["a".into(), "x".into(), "y".into(), "y".into()],
            },
            // Not in the pinned list: its app comes back to the dock.
            Group {
                id: "nosih:group:5".into(),
                apps: vec!["z".into()],
            },
            // One app only: the app takes its place.
            Group {
                id: "nosih:group:2".into(),
                apps: vec!["w".into()],
            },
        ]);
        assert_eq!(roster.pinned(), ["a", "nosih:group:1", "w", "z"]);
        assert_eq!(members(&roster, "nosih:group:1"), ["x", "y"]);
        assert!(roster.recent().is_empty());
    }

    #[test]
    fn loading_a_collapsed_group_must_not_also_show_its_app_in_recent() {
        let roster = Roster::new(
            vec!["nosih:group:1".into()],
            vec!["a".into(), "other".into()],
        )
        .with_groups(vec![Group {
            id: "nosih:group:1".into(),
            apps: vec!["a".into()],
        }]);
        assert_eq!(roster.pinned(), ["a"]);
        assert_eq!(roster.recent(), ["other"]);
    }

    #[test]
    fn recovering_an_orphaned_group_must_not_duplicate_its_apps_in_recent() {
        let roster =
            Roster::new(Vec::new(), vec!["a".into(), "b".into()]).with_groups(vec![Group {
                id: "nosih:group:1".into(),
                apps: vec!["a".into(), "b".into()],
            }]);
        assert_eq!(roster.pinned(), ["a", "b"]);
        assert!(roster.recent().is_empty());
    }
}
