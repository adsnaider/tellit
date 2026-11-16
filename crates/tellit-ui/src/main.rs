//! Browser entrypoint mounting the [`tellit_ui::App`] component.

fn main() {
    leptos::mount::mount_to_body(tellit_ui::App);
}
