//! UI components for Jujutsu revision navigation, diff rendering, and review threads.

pub mod change_overview;
pub mod comments;
pub mod file_list;
pub mod help_modal;
pub mod image_diff;
pub mod keyboard;
pub mod markdown_diff;
pub mod review_dot;
pub mod revision_selector;
pub mod revision_tree;
pub mod settings_modal;
pub mod single_file_view;
pub mod text_diff;

/// Returns `true` for an unmodified left-button click that should be handled via SPA routing.
#[must_use]
pub fn is_plain_primary_click(ev: &web_sys::MouseEvent) -> bool {
    ev.button() == 0 && !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() && !ev.alt_key()
}
