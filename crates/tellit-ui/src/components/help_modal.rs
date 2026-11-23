//! Keyboard shortcuts reference modal toggled via `?` or the header help button.

use leptos::prelude::*;

#[component]
fn ShortcutRow(keys: &'static [&'static str], label: &'static str) -> impl IntoView {
    view! {
        <div class="help-row">
            <span class="help-keys">
                {keys.iter().map(|k| view! { <kbd class="kbd-cap">{*k}</kbd> }).collect_view()}
            </span>
            <span class="help-desc">{label}</span>
        </div>
    }
}

/// Modal dialog summarizing available keyboard navigation and review shortcuts.
#[component]
pub fn HelpModal(#[prop(into)] is_open: Signal<bool>, on_close: Callback<()>) -> impl IntoView {
    move || {
        if !is_open.get() {
            return ().into_any();
        }
        view! {
            <div class="modal-backdrop" role="presentation" on:click=move |_| on_close.run(())>
                <div
                    class="modal-panel modal-panel--help"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Keyboard Shortcuts"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <header class="modal-header">
                        <h2 class="modal-title">"Keyboard Shortcuts"</h2>
                        <button
                            type="button"
                            class="icon-btn icon-btn--lg"
                            aria-label="Close help modal"
                            on:click=move |_| on_close.run(())
                        >
                            "×"
                        </button>
                    </header>

                    <div class="help-grid">
                        <section class="help-section">
                            <h3 class="help-section__title">"Line Navigation"</h3>
                            <ShortcutRow keys=&["j", "↓"] label="Next line" />
                            <ShortcutRow keys=&["k", "↑"] label="Previous line" />
                            <ShortcutRow keys=&["h", "←"] label="Focus old side (split)" />
                            <ShortcutRow keys=&["l", "→"] label="Focus new side (split)" />
                        </section>

                        <section class="help-section">
                            <h3 class="help-section__title">"Chunk & Comment Navigation"</h3>
                            <ShortcutRow keys=&["n"] label="Next change chunk" />
                            <ShortcutRow keys=&["p"] label="Previous change chunk" />
                            <ShortcutRow keys=&["Shift+N"] label="Next unresolved comment" />
                            <ShortcutRow keys=&["Shift+P"] label="Previous unresolved comment" />
                        </section>

                        <section class="help-section">
                            <h3 class="help-section__title">"File & View Navigation"</h3>
                            <ShortcutRow keys=&["u"] label="Return to change overview" />
                            <ShortcutRow keys=&["]"] label="Next file" />
                            <ShortcutRow keys=&["["] label="Previous file" />
                            <ShortcutRow keys=&["{"] label="First file" />
                            <ShortcutRow keys=&["}"] label="Last file" />
                        </section>

                        <section class="help-section">
                            <h3 class="help-section__title">"Review & View Actions"</h3>
                            <ShortcutRow keys=&["Shift+U"] label="Unified diff view" />
                            <ShortcutRow keys=&["s"] label="Split diff view" />
                            <ShortcutRow keys=&["v"] label="Toggle viewed on active file" />
                            <ShortcutRow keys=&["Shift+V"] label="Mark viewed and open next unviewed (or overview)" />
                            <ShortcutRow keys=&["c"] label="Comment on focused line" />
                            <ShortcutRow keys=&["Shift+L"] label="Toggle comments drawer" />
                            <ShortcutRow keys=&["Shift+C"] label="Copy all comments prompt" />
                            <ShortcutRow keys=&["Shift+R"] label="Refresh diff" />
                            <ShortcutRow keys=&["?"] label="Toggle this help" />
                            <ShortcutRow keys=&["Esc"] label="Close active modal or drawer" />
                        </section>
                    </div>

                    <footer class="modal-footer modal-footer--note">
                        <span class="setting-help">"Shortcuts are automatically disabled while typing in input fields or textareas."</span>
                    </footer>
                </div>
            </div>
        }
        .into_any()
    }
}
