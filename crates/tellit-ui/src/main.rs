//! Browser entrypoint mounting the [`tellit_ui::App`] component.

extern crate alloc;

fn main() {
    std::panic::set_hook(alloc::boxed::Box::new(|info| {
        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&alloc::format!(
            "tellit-ui panic: {info}"
        )));
    }));
    leptos::mount::mount_to_body(tellit_ui::App);
}
