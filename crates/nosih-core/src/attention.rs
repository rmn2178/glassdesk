//! Pending native attention requests, not unread-message counts. Each window
//! owns its request; focusing one never acknowledges another window's request.

use std::collections::HashSet;

#[derive(Default)]
pub struct Attention {
    pending: HashSet<isize>,
}

impl Attention {
    pub fn request(&mut self, window: isize, foreground: isize) -> bool {
        window != 0 && window != foreground && self.pending.insert(window)
    }

    pub fn acknowledge(&mut self, window: isize) -> bool {
        self.pending.remove(&window)
    }

    pub fn contains(&self, window: isize) -> bool {
        self.pending.contains(&window)
    }

    pub fn retain(&mut self, mut alive: impl FnMut(isize) -> bool) -> bool {
        let before = self.pending.len();
        self.pending.retain(|&window| alive(window));
        before != self.pending.len()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_idempotent_and_never_count_events_as_messages() {
        let mut attention = Attention::default();
        assert!(attention.request(7, 2));
        assert!(!attention.request(7, 2));
        assert!(attention.contains(7));
        assert!(!attention.request(2, 2));
        assert!(!attention.request(0, 2));
    }

    #[test]
    fn activating_one_window_leaves_the_others_request_pending() {
        let mut attention = Attention::default();
        attention.request(7, 2);
        attention.request(8, 2);
        assert!(attention.acknowledge(7));
        assert!(!attention.contains(7));
        assert!(attention.contains(8));
    }

    #[test]
    fn closed_windows_and_disabling_badges_leave_no_stale_requests() {
        let mut attention = Attention::default();
        attention.request(7, 2);
        attention.request(8, 2);
        assert!(attention.retain(|window| window == 8));
        assert!(!attention.contains(7));
        assert!(!attention.retain(|window| window == 8));
        attention.clear();
        assert!(!attention.contains(8));
        assert!(!attention.acknowledge(8));
    }
}
