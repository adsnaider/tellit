//! Review status indicator dot rendered beside revisions and evolog commits.

extern crate alloc;

use alloc::string::String;
use leptos::prelude::*;
use tellit_types::ReviewStatus;

/// Displays a colored review status dot (yellow unresolved, green approved, emerald clean-rebase, gray resolved).
#[component]
pub fn ReviewDot(
    #[prop(into)] status: Signal<ReviewStatus>,
    #[prop(optional)] comment_count: usize,
    #[prop(optional)] unresolved_count: usize,
    #[prop(default = None)] clean_rebase_source_number: Option<usize>,
) -> impl IntoView {
    move || {
        let current = status.get();
        let (class_name, title_text) = match current {
            ReviewStatus::HasUnresolved => {
                let suffix = if unresolved_count == 1 { "" } else { "s" };
                (
                    "review-dot review-dot--unresolved",
                    alloc::format!("{unresolved_count} unresolved comment{suffix}"),
                )
            }
            ReviewStatus::Approved => {
                let title = if comment_count > 0 {
                    String::from("Approved (all comments resolved)")
                } else {
                    String::from("Approved")
                };
                ("review-dot review-dot--approved", title)
            }
            ReviewStatus::CleanRebaseApproved => {
                let title = match clean_rebase_source_number {
                    Some(num) => alloc::format!("Approved (Clean Rebase from #{num})"),
                    None => String::from("Approved (Clean Rebase)"),
                };
                ("review-dot review-dot--clean-rebase", title)
            }
            ReviewStatus::AllResolved => (
                "review-dot review-dot--resolved",
                String::from("All comments resolved"),
            ),
            ReviewStatus::None => return None,
        };

        Some(view! {
            <span class=class_name title=title_text.clone() aria-label=title_text></span>
        })
    }
}
