//! Pure-Rust image diff viewer supporting side-by-side, swipe slider, and onion-skin modes.

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use tellit_types::{DiffFile, FileStatus};
use wasm_bindgen::JsCast;
use web_sys::HtmlImageElement;

use crate::api;

/// Visual comparison mode when both old and new versions of an image exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageCompareMode {
    /// Two side-by-side cards showing Previous vs Current.
    #[default]
    SideBySide,
    /// Layered viewport with a horizontal clip swipe slider.
    Swipe,
    /// Layered viewport with an opacity slider blending new over old.
    OnionSkin,
}

/// Natural pixel dimensions and byte payload size loaded for one image side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageMetadata {
    /// Intrinsic pixel width reported by the browser image element.
    pub width: Option<u32>,
    /// Intrinsic pixel height reported by the browser image element.
    pub height: Option<u32>,
    /// Byte length measured via `GET /api/blob/{*path}?ref=...`.
    pub bytes: Option<u64>,
    /// `true` if the image failed to load or decode.
    pub failed: bool,
}

/// Formats byte counts cleanly (`42 B`, `3.5 KB`, `1.2 MB`).
#[must_use]
pub fn format_byte_size(bytes: u64) -> String {
    if bytes < 1024 {
        alloc::format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        alloc::format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        alloc::format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Formats pixel dimensions and byte length for display beneath an image preview.
#[must_use]
pub fn format_image_meta(meta: &ImageMetadata) -> Option<String> {
    let dims = match (meta.width, meta.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some(alloc::format!("W: {w}px | H: {h}px")),
        _ => None,
    };
    let size = meta.bytes.filter(|&b| b > 0).map(format_byte_size);
    match (dims, size) {
        (Some(d), Some(s)) => Some(alloc::format!("{d} | {s}")),
        (Some(d), None) => Some(d),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

fn attach_load_metadata(
    url: String,
    meta_signal: RwSignal<ImageMetadata>,
) -> impl Fn(ev::Event) + 'static {
    move |ev: ev::Event| {
        let dims = ev
            .target()
            .and_then(|t| t.dyn_into::<HtmlImageElement>().ok())
            .map(|img| (img.natural_width(), img.natural_height()));
        let _ = meta_signal.try_update(|m| {
            m.failed = false;
            if let Some((w, h)) = dims {
                m.width = Some(w);
                m.height = Some(h);
            }
        });
        let fetch_url = url.clone();
        spawn_local(async move {
            if let Ok(bytes) = api::fetch_blob_byte_size(&fetch_url).await {
                let _ = meta_signal.try_update(|m| m.bytes = Some(bytes));
            }
        });
    }
}

#[component]
fn SingleImageCard(
    title: &'static str,
    src: String,
    alt: String,
    meta: RwSignal<ImageMetadata>,
) -> impl IntoView {
    view! {
        <div class="img-card">
            <div class="img-card__caption">{title}</div>
            <div class="img-card__frame img-checkerboard">
                {move || if meta.get().failed {
                    view! { <div class="img-error">"Image could not be loaded"</div> }.into_any()
                } else {
                    view! {
                        <img
                            src=src.clone()
                            alt=alt.clone()
                            class="img-preview"
                            on:load=attach_load_metadata(src.clone(), meta)
                            on:error=move |_| {
                                let _ = meta.try_update(|m| m.failed = true);
                            }
                        />
                    }.into_any()
                }}
            </div>
            {move || format_image_meta(&meta.get()).map(|label| view! {
                <div class="img-card__meta">{label}</div>
            })}
        </div>
    }
}

/// Interactive image diff viewer supporting single-sided, side-by-side, swipe, and onion-skin modes.
#[component]
pub fn ImageDiffBody(
    file: Arc<DiffFile>,
    base_commit: String,
    target_commit: String,
) -> impl IntoView {
    let old_meta = RwSignal::new(ImageMetadata::default());
    let new_meta = RwSignal::new(ImageMetadata::default());
    let compare_mode = RwSignal::new(ImageCompareMode::SideBySide);
    let swipe_pct = RwSignal::new(50u32);
    let opacity_pct = RwSignal::new(50u32);

    let old_path = file.old_path.clone().unwrap_or_else(|| file.path.clone());
    let old_url = api::blob_url(&old_path, &base_commit);
    let new_url = api::blob_url(&file.path, &target_commit);

    if file.status == FileStatus::Deleted {
        return view! {
            <div class="img-diff-body">
                <div class="img-status-banner img-status-banner--del">"Deleted Image"</div>
                <SingleImageCard
                    title="Previous version"
                    src=old_url
                    alt=alloc::format!("Previous version of {old_path}")
                    meta=old_meta
                />
            </div>
        }
        .into_any();
    }

    if file.status == FileStatus::Added {
        return view! {
            <div class="img-diff-body">
                <div class="img-status-banner img-status-banner--add">"Added Image"</div>
                <SingleImageCard
                    title="New file"
                    src=new_url
                    alt=alloc::format!("New image {}", file.path)
                    meta=new_meta
                />
            </div>
        }
        .into_any();
    }

    let old_alt = alloc::format!("Previous version of {old_path}");
    let new_alt = alloc::format!("Current version of {}", file.path);

    view! {
        <div class="img-diff-body">
            <div class="img-toolbar">
                <span class="img-status-banner">"Modified Image"</span>
                <div class="segmented" role="group" aria-label="Image comparison mode">
                    <button
                        type="button"
                        class=move || if compare_mode.get() == ImageCompareMode::SideBySide { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        on:click=move |_| compare_mode.set(ImageCompareMode::SideBySide)
                    >
                        "Side by Side"
                    </button>
                    <button
                        type="button"
                        class=move || if compare_mode.get() == ImageCompareMode::Swipe { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        on:click=move |_| compare_mode.set(ImageCompareMode::Swipe)
                    >
                        "Swipe"
                    </button>
                    <button
                        type="button"
                        class=move || if compare_mode.get() == ImageCompareMode::OnionSkin { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        on:click=move |_| compare_mode.set(ImageCompareMode::OnionSkin)
                    >
                        "Onion Skin"
                    </button>
                </div>
            </div>

            {move || match compare_mode.get() {
                ImageCompareMode::SideBySide => view! {
                    <div class="img-side-grid">
                        <SingleImageCard
                            title="Previous version"
                            src=old_url.clone()
                            alt=old_alt.clone()
                            meta=old_meta
                        />
                        <SingleImageCard
                            title="Current version"
                            src=new_url.clone()
                            alt=new_alt.clone()
                            meta=new_meta
                        />
                    </div>
                }.into_any(),
                ImageCompareMode::Swipe => {
                    let load_old = attach_load_metadata(old_url.clone(), old_meta);
                    let load_new = attach_load_metadata(new_url.clone(), new_meta);
                    view! {
                        <div class="img-overlay-wrap">
                            <div class="img-stage img-checkerboard">
                                <img
                                    src=old_url.clone()
                                    alt=old_alt.clone()
                                    class="img-stage__base"
                                    on:load=load_old
                                />
                                <img
                                    src=new_url.clone()
                                    alt=new_alt.clone()
                                    class="img-stage__overlay"
                                    style=move || alloc::format!("clip-path: inset(0 {}% 0 0);", 100u32.saturating_sub(swipe_pct.get()))
                                    on:load=load_new
                                />
                                <div
                                    class="img-swipe-line"
                                    style=move || alloc::format!("left: {}%;", swipe_pct.get())
                                ></div>
                                <span class="img-corner-tag img-corner-tag--left">"Before"</span>
                                <span class="img-corner-tag img-corner-tag--right">"After"</span>
                            </div>
                            <label class="img-slider-row">
                                <span>"Swipe split"</span>
                                <input
                                    type="range"
                                    min="0"
                                    max="100"
                                    prop:value=move || swipe_pct.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                                            swipe_pct.set(v.min(100));
                                        }
                                    }
                                />
                                <span class="img-slider-val">{move || alloc::format!("{}%", swipe_pct.get())}</span>
                            </label>
                            <div class="img-meta-pair">
                                {move || format_image_meta(&old_meta.get()).map(|m| view! { <span>{alloc::format!("Before: {m}")}</span> })}
                                {move || format_image_meta(&new_meta.get()).map(|m| view! { <span>{alloc::format!("After: {m}")}</span> })}
                            </div>
                        </div>
                    }.into_any()
                }
                ImageCompareMode::OnionSkin => {
                    let load_old = attach_load_metadata(old_url.clone(), old_meta);
                    let load_new = attach_load_metadata(new_url.clone(), new_meta);
                    view! {
                        <div class="img-overlay-wrap">
                            <div class="img-stage img-checkerboard">
                                <img
                                    src=old_url.clone()
                                    alt=old_alt.clone()
                                    class="img-stage__base"
                                    on:load=load_old
                                />
                                <img
                                    src=new_url.clone()
                                    alt=new_alt.clone()
                                    class="img-stage__overlay"
                                    style=move || alloc::format!("opacity: {:.2};", f64::from(opacity_pct.get()) / 100.0)
                                    on:load=load_new
                                />
                            </div>
                            <label class="img-slider-row">
                                <span>"New version opacity"</span>
                                <input
                                    type="range"
                                    min="0"
                                    max="100"
                                    prop:value=move || opacity_pct.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                                            opacity_pct.set(v.min(100));
                                        }
                                    }
                                />
                                <span class="img-slider-val">{move || alloc::format!("{}%", opacity_pct.get())}</span>
                            </label>
                            <div class="img-meta-pair">
                                {move || format_image_meta(&old_meta.get()).map(|m| view! { <span>{alloc::format!("Before: {m}")}</span> })}
                                {move || format_image_meta(&new_meta.get()).map(|m| view! { <span>{alloc::format!("After: {m}")}</span> })}
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_byte_sizes_and_dimensions() {
        assert_eq!(format_byte_size(512), "512 B");
        assert_eq!(format_byte_size(2048), "2.0 KB");
        assert_eq!(format_byte_size(2 * 1024 * 1024), "2.0 MB");

        let meta = ImageMetadata {
            width: Some(640),
            height: Some(480),
            bytes: Some(1536),
            failed: false,
        };
        assert_eq!(
            format_image_meta(&meta).as_deref(),
            Some("W: 640px | H: 480px | 1.5 KB")
        );
    }
}
