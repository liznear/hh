use crate::ui_compat::text::Line;

use crate::app::chat_state::SelectionPosition;
use crate::app::state::AppState;

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
    // Line cache key inputs: App message generation + wrap width + dirty hint.
    // Visible cache key inputs: wrap width + visible height + scroll offset
    // + selection range + line_cache_generation.
    // Invalidated by: mark_full_dirty/mark_tail_dirty, App message generation changes,
    // and width/viewport/selection changes.
    // Fallback behavior: full rebuild via build_message_lines_with_starts.
    cached_lines: Vec<Line<'static>>,
    cached_message_starts: Vec<usize>,
    cached_visible_lines: Vec<Line<'static>>,
    cached_width: usize,
    needs_rebuild: bool,
    message_dirty_hint: MessageDirtyHint,
    line_cache_generation: u64,
    visible_cache_key: Option<VisibleCacheKey>,
    app_message_generation: u64,
}

impl MessageViewportCache {
    pub fn new() -> Self {
        Self {
            cached_lines: Vec::new(),
            cached_message_starts: Vec::new(),
            cached_visible_lines: Vec::new(),
            cached_width: 0,
            needs_rebuild: true,
            message_dirty_hint: MessageDirtyHint::Full,
            line_cache_generation: 0,
            visible_cache_key: None,
            app_message_generation: 0,
        }
    }

    pub fn get_lines(&mut self, app: &AppState, width: usize) -> &Vec<Line<'static>> {
        let app_generation = app.message_cache_generation();
        if app_generation != self.app_message_generation {
            self.mark_full_dirty();
            self.app_message_generation = app_generation;
        }

        let needs_rebuild = self.needs_rebuild;
        let cached_width = self.cached_width;

        if needs_rebuild || cached_width != width {
            let mut rebuilt_incrementally = false;

            if needs_rebuild && cached_width == width {
                let hint = self.message_dirty_hint;
                if hint == MessageDirtyHint::MutateLast {
                    let message_count = app.messages.len();
                    if message_count > 0 {
                        let last_index = message_count - 1;
                        let starts = &mut self.cached_message_starts;
                        if starts.len() == message_count {
                            let last_start = starts[last_index];
                            let lines = &mut self.cached_lines;
                            if last_start <= lines.len() {
                                lines.truncate(last_start);
                                starts.truncate(last_index);
                                starts.push(lines.len());
                                crate::app::render::append_message_lines_for_index(
                                    lines, app, width, last_index,
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
                self.cached_lines = lines;
                self.cached_message_starts = starts;
            }

            self.cached_width = width;
            self.needs_rebuild = false;
            self.message_dirty_hint = MessageDirtyHint::Full;
            self.line_cache_generation = self.line_cache_generation.saturating_add(1);
            self.visible_cache_key = None;
        }

        &self.cached_lines
    }

    pub fn get_visible_lines(
        &mut self,
        app: &AppState,
        wrap_width: usize,
        visible_height: usize,
        scroll_offset: usize,
    ) -> &Vec<Line<'static>> {
        self.get_lines(app, wrap_width);

        let key = VisibleCacheKey {
            wrap_width,
            visible_height,
            scroll_offset,
            selection: app.text_selection.get_range(),
            line_cache_generation: self.line_cache_generation,
        };

        if self.visible_cache_key.as_ref() != Some(&key) {
            let lines = &self.cached_lines;
            let visible_end = scroll_offset
                .saturating_add(visible_height)
                .min(lines.len());
            let mut rendered = lines[scroll_offset..visible_end].to_vec();
            crate::app::components::messages::apply_selection_highlight(
                &mut rendered,
                app,
                scroll_offset,
            );

            self.cached_visible_lines = rendered;
            self.visible_cache_key = Some(key);
        }

        &self.cached_visible_lines
    }

    pub fn mark_full_dirty(&mut self) {
        self.needs_rebuild = true;
        self.message_dirty_hint = MessageDirtyHint::Full;
    }

    pub fn mark_tail_dirty(&mut self) {
        if !(self.needs_rebuild && self.message_dirty_hint == MessageDirtyHint::Full) {
            self.message_dirty_hint = MessageDirtyHint::MutateLast;
        }
        self.needs_rebuild = true;
    }
}

impl Default for MessageViewportCache {
    fn default() -> Self {
        Self::new()
    }
}
