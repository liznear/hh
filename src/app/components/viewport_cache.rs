use std::cell::{Cell, Ref, RefCell};

use ratatui::text::Line;

use crate::app::chat_state::{ChatApp, SelectionPosition};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageDirtyHint {
    Full,
    MutateLast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisibleCacheKey {
    wrap_width: usize,
    visible_height: usize,
    scroll_offset: usize,
    selection: Option<(SelectionPosition, SelectionPosition)>,
    line_cache_generation: u64,
}

pub struct MessageViewportCache {
    cached_lines: RefCell<Vec<Line<'static>>>,
    cached_message_starts: RefCell<Vec<usize>>,
    cached_visible_lines: RefCell<Vec<Line<'static>>>,
    cached_width: Cell<usize>,
    needs_rebuild: Cell<bool>,
    message_dirty_hint: Cell<MessageDirtyHint>,
    line_cache_generation: Cell<u64>,
    visible_cache_key: RefCell<Option<VisibleCacheKey>>,
}

impl MessageViewportCache {
    pub fn new() -> Self {
        Self {
            cached_lines: RefCell::new(Vec::new()),
            cached_message_starts: RefCell::new(Vec::new()),
            cached_visible_lines: RefCell::new(Vec::new()),
            cached_width: Cell::new(0),
            needs_rebuild: Cell::new(true),
            message_dirty_hint: Cell::new(MessageDirtyHint::Full),
            line_cache_generation: Cell::new(0),
            visible_cache_key: RefCell::new(None),
        }
    }

    pub fn get_lines<'a>(&'a self, app: &ChatApp, width: usize) -> Ref<'a, Vec<Line<'static>>> {
        let needs_rebuild = self.needs_rebuild.get();
        let cached_width = self.cached_width.get();

        if needs_rebuild || cached_width != width {
            let mut rebuilt_incrementally = false;

            if needs_rebuild && cached_width == width {
                let hint = self.message_dirty_hint.get();
                if hint == MessageDirtyHint::MutateLast {
                    let message_count = app.messages.len();
                    if message_count > 0 {
                        let last_index = message_count - 1;
                        let mut starts = self.cached_message_starts.borrow_mut();
                        if starts.len() == message_count {
                            let last_start = starts[last_index];
                            let mut lines = self.cached_lines.borrow_mut();
                            if last_start <= lines.len() {
                                lines.truncate(last_start);
                                starts.truncate(last_index);
                                starts.push(lines.len());
                                crate::app::render::append_message_lines_for_index(
                                    &mut lines, app, width, last_index,
                                );
                                rebuilt_incrementally = true;
                            }
                        }
                    }
                }
            }

            if !rebuilt_incrementally {
                let (lines, starts) =
                    crate::app::render::build_message_lines_with_starts(app, width);
                *self.cached_lines.borrow_mut() = lines;
                *self.cached_message_starts.borrow_mut() = starts;
            }

            self.cached_width.set(width);
            self.needs_rebuild.set(false);
            self.message_dirty_hint.set(MessageDirtyHint::Full);
            self.line_cache_generation
                .set(self.line_cache_generation.get().saturating_add(1));
            *self.visible_cache_key.borrow_mut() = None;
        }

        self.cached_lines.borrow()
    }

    pub fn get_visible_lines<'a>(
        &'a self,
        app: &ChatApp,
        wrap_width: usize,
        visible_height: usize,
        scroll_offset: usize,
    ) -> Ref<'a, Vec<Line<'static>>> {
        {
            let lines = self.get_lines(app, wrap_width);
            drop(lines);
        }

        let key = VisibleCacheKey {
            wrap_width,
            visible_height,
            scroll_offset,
            selection: app.text_selection.get_range(),
            line_cache_generation: self.line_cache_generation.get(),
        };

        if self.visible_cache_key.borrow().as_ref() != Some(&key) {
            let lines = self.cached_lines.borrow();
            let visible_end = scroll_offset
                .saturating_add(visible_height)
                .min(lines.len());
            let mut rendered = lines[scroll_offset..visible_end].to_vec();
            crate::app::render::apply_selection_highlight(&mut rendered, app, scroll_offset);
            drop(lines);

            *self.cached_visible_lines.borrow_mut() = rendered;
            *self.visible_cache_key.borrow_mut() = Some(key);
        }

        self.cached_visible_lines.borrow()
    }

    pub fn mark_full_dirty(&self) {
        self.needs_rebuild.set(true);
        self.message_dirty_hint.set(MessageDirtyHint::Full);
    }

    pub fn mark_tail_dirty(&self) {
        if !(self.needs_rebuild.get() && self.message_dirty_hint.get() == MessageDirtyHint::Full) {
            self.message_dirty_hint.set(MessageDirtyHint::MutateLast);
        }
        self.needs_rebuild.set(true);
    }
}

impl Default for MessageViewportCache {
    fn default() -> Self {
        Self::new()
    }
}
