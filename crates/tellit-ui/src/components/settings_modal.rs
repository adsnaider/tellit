//! Appearance and system preferences modal backed by `localStorage` and `/api/user-settings`.

extern crate alloc;

use alloc::string::{String, ToString};

use leptos::prelude::*;
use tellit_types::{
    AppearanceSettings, CUSTOM_EDITOR_ID, ColorVisionMode, EDITOR_PRESETS, EditorSpecRequest,
    NONE_EDITOR_ID, ThemePreference, resolve_editor_preset,
};

use crate::state;

const DEFAULT_EDITOR_ID: &str = "vscode";

fn editor_id_or_default(settings: &AppearanceSettings) -> &str {
    settings.editor.id.as_deref().unwrap_or(DEFAULT_EDITOR_ID)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    Appearance,
    System,
}

const FONT_FAMILIES: &[(&str, &str)] = &[
    (
        "System Font",
        r#"-apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif"#,
    ),
    ("Menlo", r#"Menlo, Monaco, "Courier New", monospace"#),
    (
        "SF Mono",
        r#""SF Mono", Consolas, "Liberation Mono", monospace"#,
    ),
    ("Fira Code", r#""Fira Code", "Courier New", monospace"#),
    (
        "JetBrains Mono",
        r#""JetBrains Mono", "Courier New", monospace"#,
    ),
];

/// Modal dialog for configuring theme, font, color vision, editor command, and auto-viewed globs.
#[component]
pub fn SettingsModal(
    #[prop(into)] is_open: Signal<bool>,
    #[prop(into)] settings: Signal<AppearanceSettings>,
    on_change: Callback<AppearanceSettings>,
    on_close: Callback<()>,
) -> impl IntoView {
    let active_section = RwSignal::new(SettingsSection::Appearance);
    let patterns_draft = RwSignal::new(state::format_auto_viewed_patterns(
        &settings.get_untracked().auto_viewed_patterns,
    ));

    let is_none_memo = Memo::new(move |_| {
        settings.with(|s| editor_id_or_default(s).eq_ignore_ascii_case(NONE_EDITOR_ID))
    });
    let is_custom_memo = Memo::new(move |_| {
        settings.with(|s| editor_id_or_default(s).eq_ignore_ascii_case(CUSTOM_EDITOR_ID))
    });
    let cmd_invalid_memo = Memo::new(move |_| {
        is_custom_memo.get()
            && settings.with(|s| s.editor.command.as_deref().unwrap_or("").trim().is_empty())
    });
    let args_invalid_memo = Memo::new(move |_| {
        is_custom_memo.get()
            && settings.with(|s| {
                s.editor
                    .args_template
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            })
    });

    Effect::new(move |_| {
        if is_open.get() {
            let current = settings.get_untracked();
            patterns_draft.set(state::format_auto_viewed_patterns(
                &current.auto_viewed_patterns,
            ));
        }
    });

    move || {
        if !is_open.get() {
            return ().into_any();
        }

        view! {
            <div class="modal-backdrop" role="presentation" on:click=move |_| on_close.run(())>
                <div
                    class="modal-panel modal-panel--settings"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Settings"
                    on:click=move |ev| ev.stop_propagation()
                >
                    <header class="modal-header">
                        <h2 class="modal-title">"Settings"</h2>
                        <button
                            type="button"
                            class="icon-btn icon-btn--lg"
                            aria-label="Close settings"
                            on:click=move |_| on_close.run(())
                        >
                            "×"
                        </button>
                    </header>

                    <div class="settings-body">
                        <nav class="settings-nav" aria-label="Settings sections">
                            <button
                                type="button"
                                class=move || if active_section.get() == SettingsSection::Appearance { "settings-nav__btn is-active" } else { "settings-nav__btn" }
                                on:click=move |_| active_section.set(SettingsSection::Appearance)
                            >
                                "Appearance"
                            </button>
                            <button
                                type="button"
                                class=move || if active_section.get() == SettingsSection::System { "settings-nav__btn is-active" } else { "settings-nav__btn" }
                                on:click=move |_| active_section.set(SettingsSection::System)
                            >
                                "System"
                            </button>
                        </nav>

                        <div class="settings-content">
                            {move || match active_section.get() {
                                SettingsSection::Appearance => view! {
                                    <div class="settings-group">
                                        <div class="setting-field">
                                            <label class="setting-label" for="settings-font-size">
                                                "Font Size"
                                            </label>
                                            <div class="setting-range-row">
                                                <input
                                                    id="settings-font-size"
                                                    type="range"
                                                    min="10"
                                                    max="20"
                                                    step="1"
                                                    prop:value=move || settings.with(|s| s.font_size.to_string())
                                                    on:input=move |ev| {
                                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                                            let mut next = settings.get_untracked();
                                                            next.font_size = v.clamp(10, 20);
                                                            on_change.run(next);
                                                        }
                                                    }
                                                />
                                                <span class="setting-range-val">{move || settings.with(|s| alloc::format!("{}px", s.font_size))}</span>
                                            </div>
                                        </div>

                                        <div class="setting-field">
                                            <label class="setting-label" for="settings-font-family">
                                                "Font Family"
                                            </label>
                                            <select
                                                id="settings-font-family"
                                                class="select-input"
                                                prop:value=move || settings.with(|s| s.font_family.clone())
                                                on:change=move |ev| {
                                                    let mut next = settings.get_untracked();
                                                    next.font_family = event_target_value(&ev);
                                                    on_change.run(next);
                                                }
                                            >
                                                {FONT_FAMILIES.iter().map(|(label, val)| {
                                                    let opt_val = *val;
                                                    view! {
                                                        <option
                                                            value=opt_val
                                                            selected=move || settings.with(|s| s.font_family == opt_val)
                                                        >
                                                            {*label}
                                                        </option>
                                                    }
                                                }).collect_view()}
                                            </select>
                                        </div>

                                        <div class="setting-field">
                                            <span class="setting-label">"Theme"</span>
                                            <div class="setting-btn-row">
                                                {[
                                                    (ThemePreference::Dark, "Dark"),
                                                    (ThemePreference::Light, "Light"),
                                                    (ThemePreference::Auto, "Auto"),
                                                ].into_iter().map(|(pref, label)| {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            class=move || if settings.with(|s| s.theme == pref) { "btn btn--active-accent" } else { "btn" }
                                                            on:click=move |_| {
                                                                let mut next = settings.get_untracked();
                                                                next.theme = pref;
                                                                on_change.run(next);
                                                            }
                                                        >
                                                            {label}
                                                        </button>
                                                    }
                                                }).collect_view()}
                                            </div>
                                        </div>

                                        <div class="setting-field">
                                            <span class="setting-label">"Color Vision"</span>
                                            <div class="setting-btn-row">
                                                {[
                                                    (ColorVisionMode::Normal, "Normal"),
                                                    (ColorVisionMode::Deuteranopia, "Deuteranopia"),
                                                ].into_iter().map(|(mode, label)| {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            class=move || if settings.with(|s| s.color_vision == mode) { "btn btn--active-accent" } else { "btn" }
                                                            title="Deuteranopia mode uses blue/orange instead of green/red for diffs"
                                                            on:click=move |_| {
                                                                let mut next = settings.get_untracked();
                                                                next.color_vision = mode;
                                                                on_change.run(next);
                                                            }
                                                        >
                                                            {label}
                                                        </button>
                                                    }
                                                }).collect_view()}
                                            </div>
                                        </div>
                                    </div>
                                }.into_any(),
                                SettingsSection::System => view! {
                                    <div class="settings-group">
                                        <div class="setting-field">
                                            <label class="setting-label" for="auto-viewed-patterns">
                                                "Auto-Mark Viewed Patterns"
                                            </label>
                                            <p class="setting-help">
                                                "Files matching these glob patterns start marked as viewed (one pattern per line)."
                                            </p>
                                            <textarea
                                                id="auto-viewed-patterns"
                                                class="textarea-input textarea-input--mono"
                                                rows=5
                                                spellcheck="false"
                                                placeholder="*.lock\n*.stories.tsx\nsrc/generated/**"
                                                prop:value=move || patterns_draft.get()
                                                on:input=move |ev| {
                                                    let val = event_target_value(&ev);
                                                    patterns_draft.set(val.clone());
                                                    let mut next = settings.get_untracked();
                                                    next.auto_viewed_patterns = state::parse_auto_viewed_patterns(&val);
                                                    on_change.run(next);
                                                }
                                            />
                                        </div>

                                        <div class="setting-field">
                                            <label class="setting-label" for="editor-preset-select">
                                                "Open In Editor"
                                            </label>
                                            <select
                                                id="editor-preset-select"
                                                class="select-input"
                                                prop:value=move || settings.with(|s| String::from(editor_id_or_default(s)))
                                                on:change=move |ev| {
                                                    let next_id = event_target_value(&ev);
                                                    let mut next = settings.get_untracked();
                                                    if next_id == NONE_EDITOR_ID {
                                                        next.editor = EditorSpecRequest {
                                                            id: Some(String::from(NONE_EDITOR_ID)),
                                                            command: Some(String::new()),
                                                            args_template: Some(String::new()),
                                                        };
                                                    } else if next_id == CUSTOM_EDITOR_ID {
                                                        let cur_cmd = if next.editor.id.as_deref() == Some(CUSTOM_EDITOR_ID) {
                                                            next.editor.command.clone().unwrap_or_default()
                                                        } else {
                                                            String::new()
                                                        };
                                                        let cur_args = if next.editor.id.as_deref() == Some(CUSTOM_EDITOR_ID) {
                                                            next.editor.args_template.clone().unwrap_or_default()
                                                        } else {
                                                            String::new()
                                                        };
                                                        next.editor = EditorSpecRequest {
                                                            id: Some(String::from(CUSTOM_EDITOR_ID)),
                                                            command: Some(cur_cmd),
                                                            args_template: Some(cur_args),
                                                        };
                                                    } else if let Some(preset) = resolve_editor_preset(&next_id) {
                                                        next.editor = EditorSpecRequest::from_preset(*preset);
                                                    }
                                                    on_change.run(next);
                                                }
                                            >
                                                {EDITOR_PRESETS.iter().map(|preset| {
                                                    let preset_id = preset.id;
                                                    view! {
                                                        <option
                                                            value=preset_id
                                                            selected=move || settings.with(|s| editor_id_or_default(s).eq_ignore_ascii_case(preset_id))
                                                        >
                                                            {preset.label}
                                                        </option>
                                                    }
                                                }).collect_view()}
                                                <option value=CUSTOM_EDITOR_ID selected=move || is_custom_memo.get()>"Custom..."</option>
                                                <option value=NONE_EDITOR_ID selected=move || is_none_memo.get()>"Disabled"</option>
                                            </select>

                                            {move || (!is_none_memo.get()).then(|| view! {
                                                <div class="editor-spec-box">
                                                    <label class="setting-sublabel" for="editor-command-input">"Command"</label>
                                                    <input
                                                        id="editor-command-input"
                                                        type="text"
                                                        class=move || if cmd_invalid_memo.get() { "text-input text-input--mono text-input--error" } else { "text-input text-input--mono" }
                                                        disabled=move || !is_custom_memo.get()
                                                        placeholder="e.g. hx"
                                                        spellcheck="false"
                                                        prop:value=move || settings.with(|s| s.editor.command.clone().unwrap_or_default())
                                                        on:input=move |ev| {
                                                            let mut next = settings.get_untracked();
                                                            next.editor.command = Some(event_target_value(&ev));
                                                            on_change.run(next);
                                                        }
                                                    />
                                                    {move || cmd_invalid_memo.get().then(|| view! {
                                                        <p class="field-error">"Command is required."</p>
                                                    })}

                                                    <label class="setting-sublabel" for="editor-args-input">"Arguments"</label>
                                                    <input
                                                        id="editor-args-input"
                                                        type="text"
                                                        class=move || if args_invalid_memo.get() { "text-input text-input--mono text-input--error" } else { "text-input text-input--mono" }
                                                        disabled=move || !is_custom_memo.get()
                                                        placeholder="-l %line %file"
                                                        spellcheck="false"
                                                        prop:value=move || settings.with(|s| s.editor.args_template.clone().unwrap_or_default())
                                                        on:input=move |ev| {
                                                            let mut next = settings.get_untracked();
                                                            next.editor.args_template = Some(event_target_value(&ev));
                                                            on_change.run(next);
                                                        }
                                                    />
                                                    {move || args_invalid_memo.get().then(|| view! {
                                                        <p class="field-error">"Arguments are required."</p>
                                                    })}
                                                    <p class="setting-help">
                                                        {move || if is_custom_memo.get() {
                                                            "Use %file and %line placeholders. Passed directly without a shell."
                                                        } else {
                                                            "Select Custom... above to customize command and arguments."
                                                        }}
                                                    </p>
                                                </div>
                                            })}
                                        </div>
                                    </div>
                                }.into_any(),
                            }}
                        </div>
                    </div>

                    <footer class="modal-footer">
                        <button
                            type="button"
                            class="btn btn--ghost"
                            on:click=move |_| {
                                let defaults = AppearanceSettings::default();
                                let mut next = settings.get_untracked();
                                if active_section.get_untracked() == SettingsSection::Appearance {
                                    next.font_size = defaults.font_size;
                                    next.font_family = defaults.font_family;
                                    next.theme = defaults.theme;
                                    next.color_vision = defaults.color_vision;
                                } else {
                                    next.editor = defaults.editor;
                                    next.auto_viewed_patterns = defaults.auto_viewed_patterns.clone();
                                    patterns_draft.set(state::format_auto_viewed_patterns(
                                        &defaults.auto_viewed_patterns,
                                    ));
                                }
                                on_change.run(next);
                            }
                        >
                            {move || if active_section.get() == SettingsSection::Appearance {
                                "Reset Appearance Defaults"
                            } else {
                                "Reset System Defaults"
                            }}
                        </button>
                        <button type="button" class="btn btn--active-accent" on:click=move |_| on_close.run(())>
                            "Close"
                        </button>
                    </footer>
                </div>
            </div>
        }
        .into_any()
    }
}
