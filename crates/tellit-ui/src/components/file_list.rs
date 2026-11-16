//! Collapsible hierarchical changed-file sidebar with status badges, viewed checkboxes, and generated collapse.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use std::collections::HashSet;

use leptos::prelude::*;
use tellit_types::DiffFile;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

/// Node inside the collapsed hierarchical changed-file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeNode {
    /// Segment or collapsed folder chain name (`"crates/tellit-ui/src"`).
    pub name: String,
    /// Full repository-relative path to this folder or file.
    pub path: String,
    /// `true` for directories, `false` for file leaves.
    pub is_directory: bool,
    /// Child nodes when `is_directory` is `true`.
    pub children: Vec<FileTreeNode>,
    /// Diff metadata when `is_directory` is `false`.
    pub file: Option<DiffFile>,
}

/// Builds a hierarchical tree from `files`, collapsing single-child directory chains.
#[must_use]
pub fn build_file_tree(files: &[DiffFile]) -> FileTreeNode {
    let mut root = FileTreeNode {
        name: String::new(),
        path: String::new(),
        is_directory: true,
        children: Vec::new(),
        file: None,
    };

    for file in files {
        let parts: Vec<&str> = file.path.split('/').filter(|s| !s.is_empty()).collect();
        insert_path_parts(&mut root, &parts, 0, file);
    }

    collapse_single_child_dirs(root)
}

fn insert_path_parts(node: &mut FileTreeNode, parts: &[&str], idx: usize, file: &DiffFile) {
    if idx >= parts.len() {
        return;
    }
    let part = parts[idx];
    let is_last = idx + 1 == parts.len();
    let path_so_far = parts[..=idx].join("/");

    if let Some(pos) = node
        .children
        .iter()
        .position(|c| c.name == part && c.is_directory == !is_last)
    {
        if !is_last {
            insert_path_parts(&mut node.children[pos], parts, idx + 1, file);
        }
        return;
    }

    let mut child = FileTreeNode {
        name: String::from(part),
        path: path_so_far,
        is_directory: !is_last,
        children: Vec::new(),
        file: if is_last { Some(file.clone()) } else { None },
    };
    if !is_last {
        insert_path_parts(&mut child, parts, idx + 1, file);
    }
    node.children.push(child);
}

fn collapse_single_child_dirs(mut node: FileTreeNode) -> FileTreeNode {
    if !node.is_directory {
        return node;
    }
    node.children = node
        .children
        .into_iter()
        .map(collapse_single_child_dirs)
        .collect();

    if !node.name.is_empty() && node.children.len() == 1 && node.children[0].is_directory {
        let child = node.children.remove(0);
        return FileTreeNode {
            name: alloc::format!("{}/{}", node.name, child.name),
            path: child.path,
            is_directory: true,
            children: child.children,
            file: None,
        };
    }

    node
}

/// Collects every directory path in `node`.
#[must_use]
pub fn get_all_directory_paths(node: &FileTreeNode) -> Vec<String> {
    let mut paths = Vec::new();
    collect_dir_paths(node, &mut paths);
    paths
}

fn collect_dir_paths(node: &FileTreeNode, out: &mut Vec<String>) {
    if !node.is_directory {
        return;
    }
    if !node.path.is_empty() {
        out.push(node.path.clone());
    }
    for child in &node.children {
        collect_dir_paths(child, out);
    }
}

/// Computes which directory paths have every descendant file marked reviewed.
#[must_use]
pub fn get_reviewed_directory_paths(
    node: &FileTreeNode,
    reviewed_files: &HashSet<String>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    visit_reviewed(node, reviewed_files, &mut out);
    out
}

fn visit_reviewed(
    node: &FileTreeNode,
    reviewed_files: &HashSet<String>,
    out: &mut HashSet<String>,
) -> bool {
    if let Some(ref file) = node.file {
        return reviewed_files.contains(&file.path);
    }
    if !node.is_directory || node.children.is_empty() {
        return false;
    }
    let mut all_done = true;
    for child in &node.children {
        // Must visit every subtree so nested reviewed directories are recorded even when a sibling is unreviewed.
        if !visit_reviewed(child, reviewed_files, out) {
            all_done = false;
        }
    }
    if all_done && !node.path.is_empty() {
        out.insert(node.path.clone());
    }
    all_done
}

/// Filters `node` by case-insensitive substring match against `query`.
#[must_use]
pub fn filter_file_tree(node: &FileTreeNode, query: &str) -> FileTreeNode {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return node.clone();
    }
    filter_tree_inner(node, &normalized).unwrap_or_else(|| FileTreeNode {
        name: node.name.clone(),
        path: node.path.clone(),
        is_directory: true,
        children: Vec::new(),
        file: None,
    })
}

fn filter_tree_inner(node: &FileTreeNode, query_lower: &str) -> Option<FileTreeNode> {
    if node.is_directory {
        let children: Vec<FileTreeNode> = node
            .children
            .iter()
            .filter_map(|child| filter_tree_inner(child, query_lower))
            .collect();
        if children.is_empty() {
            return None;
        }
        Some(FileTreeNode {
            name: node.name.clone(),
            path: node.path.clone(),
            is_directory: true,
            children,
            file: None,
        })
    } else if let Some(ref file) = node.file
        && file.path.to_ascii_lowercase().contains(query_lower)
    {
        Some(node.clone())
    } else {
        None
    }
}

/// Returns the CSS class list for a diff file's status badge.
#[must_use]
pub fn status_badge_class(status: tellit_types::FileStatus) -> &'static str {
    match status {
        tellit_types::FileStatus::Added => "file-status file-status--added",
        tellit_types::FileStatus::Modified => "file-status file-status--modified",
        tellit_types::FileStatus::Deleted => "file-status file-status--deleted",
        tellit_types::FileStatus::Renamed => "file-status file-status--renamed",
    }
}

struct TreeRenderCtx<'a> {
    expanded_dirs: RwSignal<HashSet<String>>,
    reviewed_dirs: &'a HashSet<String>,
    reviewed_files: &'a HashSet<String>,
    comment_counts: &'a std::collections::HashMap<String, usize>,
    selected_file_path: Option<&'a str>,
    on_select_file: Callback<String>,
    on_toggle_reviewed: Callback<String>,
    on_toggle_folder_reviewed: Callback<(String, bool)>,
}

fn render_tree_node(node: FileTreeNode, depth: usize, ctx: &TreeRenderCtx<'_>) -> AnyView {
    if node.is_directory {
        let dir_path = node.path.clone();
        let dir_path_toggle = dir_path.clone();
        let dir_path_check = dir_path.clone();
        let expanded_dirs = ctx.expanded_dirs;
        let on_toggle_folder_reviewed = ctx.on_toggle_folder_reviewed;
        let is_expanded = dir_path.is_empty() || expanded_dirs.with(|set| set.contains(&dir_path));
        let is_reviewed = ctx.reviewed_dirs.contains(&dir_path);
        let padding_px = depth * 14 + 12;

        let children_views = if is_expanded {
            let next_depth = if node.name.is_empty() {
                depth
            } else {
                depth + 1
            };
            node.children
                .into_iter()
                .map(|child| render_tree_node(child, next_depth, ctx))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        return view! {
            <div class="file-tree-dir-group">
                {(!node.name.is_empty()).then(|| {
                    let dir_name = node.name.clone();
                    let dir_title = dir_name.clone();
                    view! {
                    <div
                        class={if is_reviewed {
                            "file-tree-dir file-tree-dir--reviewed"
                        } else {
                            "file-tree-dir"
                        }}
                        style=alloc::format!("padding-left: {padding_px}px")
                        on:click=move |_| {
                            expanded_dirs.update(|set| {
                                if !set.remove(&dir_path_toggle) {
                                    set.insert(dir_path_toggle.clone());
                                }
                            });
                        }
                    >
                        <span class="chevron">{if is_expanded { "▾" } else { "▸" }}</span>
                        <input
                            type="checkbox"
                            class="file-checkbox"
                            prop:checked=is_reviewed
                            title={if is_reviewed {
                                "Mark folder files as not viewed"
                            } else {
                                "Mark folder files as viewed"
                            }}
                            on:click=move |ev| {
                                ev.stop_propagation();
                            }
                            on:change=move |_| {
                                on_toggle_folder_reviewed.run((dir_path_check.clone(), !is_reviewed));
                            }
                        />
                        <span class="file-tree-dir__name" title=dir_title>{dir_name}</span>
                    </div>
                }})}
                {children_views}
            </div>
        }
        .into_any();
    }

    let Some(file) = node.file else {
        return ().into_any();
    };
    let file_path = file.path.clone();
    let file_path_click = file_path.clone();
    let file_path_check = file_path.clone();
    let on_select_file = ctx.on_select_file;
    let on_toggle_reviewed = ctx.on_toggle_reviewed;
    let is_reviewed = ctx.reviewed_files.contains(&file_path);
    let is_selected = ctx.selected_file_path == Some(file_path.as_str());
    let comment_count = ctx.comment_counts.get(&file_path).copied().unwrap_or(0);
    let padding_px = depth * 14 + 12;

    let row_class = match (is_selected, is_reviewed) {
        (true, true) => "file-tree-row file-tree-row--selected file-tree-row--reviewed",
        (true, false) => "file-tree-row file-tree-row--selected",
        (false, true) => "file-tree-row file-tree-row--reviewed",
        (false, false) => "file-tree-row",
    };

    view! {
        <div
            class=row_class
            style=alloc::format!("padding-left: {padding_px}px")
            title=file_path.clone()
            on:click=move |_| on_select_file.run(file_path_click.clone())
        >
            <input
                type="checkbox"
                class="file-checkbox"
                prop:checked=is_reviewed
                title={if is_reviewed { "Mark as not viewed" } else { "Mark as viewed" }}
                on:click=move |ev| {
                    ev.stop_propagation();
                }
                on:change=move |_| {
                    on_toggle_reviewed.run(file_path_check.clone());
                }
            />
            <span class=status_badge_class(file.status)>{file.status.short_code()}</span>
            <span class="file-tree-row__name">{node.name}</span>
            <span class="file-tree-row__stats">
                {(file.additions > 0).then(|| view! {
                    <span class="stat-add">{alloc::format!("+{}", file.additions)}</span>
                })}
                {(file.deletions > 0).then(|| view! {
                    <span class="stat-del">{alloc::format!("-{}", file.deletions)}</span>
                })}
            </span>
            {(comment_count > 0).then(|| view! {
                <span class="file-comment-count" title="Comment threads">{comment_count}</span>
            })}
        </div>
    }
    .into_any()
}

/// Hierarchical changed-files sidebar with filter, status badges, `+N -M`, viewed state, and generated collapse.
#[component]
pub fn FileList(
    #[prop(into)] files: Signal<Vec<DiffFile>>,
    #[prop(into)] comment_counts: Signal<std::collections::HashMap<String, usize>>,
    #[prop(into)] reviewed_files: Signal<HashSet<String>>,
    #[prop(into)] selected_file_path: Signal<Option<String>>,
    on_select_file: Callback<String>,
    on_toggle_reviewed: Callback<String>,
    on_toggle_folder_reviewed: Callback<(String, bool)>,
) -> impl IntoView {
    let filter_text = RwSignal::new(String::new());
    let show_generated = RwSignal::new(false);
    let expanded_dirs = RwSignal::new(HashSet::<String>::new());

    let primary_files = Memo::new(move |_| {
        files
            .get()
            .into_iter()
            .filter(|f| !f.is_generated)
            .collect::<Vec<_>>()
    });
    let generated_files = Memo::new(move |_| {
        files
            .get()
            .into_iter()
            .filter(|f| f.is_generated)
            .collect::<Vec<_>>()
    });

    let primary_tree = Memo::new(move |_| build_file_tree(&primary_files.get()));
    let generated_tree = Memo::new(move |_| build_file_tree(&generated_files.get()));

    Effect::new(move |_| {
        let mut all_dirs = get_all_directory_paths(&primary_tree.get());
        all_dirs.extend(get_all_directory_paths(&generated_tree.get()));
        expanded_dirs.set(all_dirs.into_iter().collect());
    });

    let totals = Signal::derive(move || {
        files.get().iter().fold((0u32, 0u32), |(add, del), f| {
            (add + f.additions, del + f.deletions)
        })
    });

    let toggle_all_dirs = move |_| {
        let mut all_dirs = get_all_directory_paths(&primary_tree.get_untracked());
        all_dirs.extend(get_all_directory_paths(&generated_tree.get_untracked()));
        let is_all_expanded = !all_dirs.is_empty()
            && expanded_dirs.with_untracked(|cur| all_dirs.iter().all(|d| cur.contains(d)));
        if is_all_expanded {
            expanded_dirs.set(HashSet::new());
        } else {
            expanded_dirs.set(all_dirs.into_iter().collect());
        }
    };

    view! {
        <section class="file-list-panel" aria-label="Changed files">
            <div class="file-list-header">
                <div class="file-list-header__top">
                    <h3 class="file-list-title">
                        {move || alloc::format!("Files changed ({})", files.get().len())}
                    </h3>
                    <div class="file-list-header__actions">
                        <span class="diff-totals">
                            {move || {
                                let (add, del) = totals.get();
                                view! {
                                    <span class="stat-add">{alloc::format!("+{add}")}</span>
                                    <span class="stat-del">{alloc::format!("-{del}")}</span>
                                }
                            }}
                        </span>
                        <button
                            type="button"
                            class="icon-btn"
                            title="Expand or collapse all folders"
                            on:click=toggle_all_dirs
                        >
                            "⇅"
                        </button>
                    </div>
                </div>
                <input
                    type="text"
                    class="file-filter-input"
                    placeholder="Filter files..."
                    prop:value=move || filter_text.get()
                    on:input=move |ev| {
                        if let Some(target) = ev.target()
                            && let Ok(input) = target.dyn_into::<HtmlInputElement>()
                        {
                            filter_text.set(input.value());
                        }
                    }
                />
            </div>

            <div class="file-list-scroll">
                {move || {
                    let query = filter_text.get();
                    let reviewed = reviewed_files.get();
                    let counts = comment_counts.get();
                    let selected = selected_file_path.get();

                    let p_tree = filter_file_tree(&primary_tree.get(), &query);
                    let p_reviewed_dirs = get_reviewed_directory_paths(&p_tree, &reviewed);
                    let p_ctx = TreeRenderCtx {
                        expanded_dirs,
                        reviewed_dirs: &p_reviewed_dirs,
                        reviewed_files: &reviewed,
                        comment_counts: &counts,
                        selected_file_path: selected.as_deref(),
                        on_select_file,
                        on_toggle_reviewed,
                        on_toggle_folder_reviewed,
                    };
                    let primary_children = p_tree
                        .children
                        .into_iter()
                        .map(|child| render_tree_node(child, 0, &p_ctx))
                        .collect::<Vec<_>>();

                    let g_count = generated_files.get().len();
                    let g_tree = filter_file_tree(&generated_tree.get(), &query);
                    let g_reviewed_dirs = get_reviewed_directory_paths(&g_tree, &reviewed);
                    let g_ctx = TreeRenderCtx {
                        expanded_dirs,
                        reviewed_dirs: &g_reviewed_dirs,
                        reviewed_files: &reviewed,
                        comment_counts: &counts,
                        selected_file_path: selected.as_deref(),
                        on_select_file,
                        on_toggle_reviewed,
                        on_toggle_folder_reviewed,
                    };
                    let has_filter = !query.trim().is_empty();
                    let show_gen_children = show_generated.get() || (has_filter && !g_tree.children.is_empty());

                    let generated_section = (g_count > 0).then(|| {
                        let gen_children = if show_gen_children {
                            g_tree
                                .children
                                .into_iter()
                                .map(|child| render_tree_node(child, 0, &g_ctx))
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };

                        view! {
                            <div class="generated-files-group">
                                <button
                                    type="button"
                                    class="generated-files-toggle"
                                    aria-expanded=show_gen_children
                                    on:click=move |_| show_generated.update(|v| *v = !*v)
                                >
                                    <span class="chevron">{if show_gen_children { "▾" } else { "▸" }}</span>
                                    <span>{alloc::format!("Generated files ({g_count})")}</span>
                                </button>
                                {gen_children}
                            </div>
                        }
                    });

                    view! {
                        <div>
                            {primary_children}
                            {generated_section}
                        </div>
                    }
                }}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use tellit_types::FileStatus;

    fn sample_file(path: &str) -> DiffFile {
        DiffFile {
            path: String::from(path),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_generated: false,
            chunks: Vec::new(),
        }
    }

    #[test]
    fn collapses_single_child_directory_chains_and_tracks_reviewed_dirs() {
        let files = vec![
            sample_file("crates/tellit-ui/src/lib.rs"),
            sample_file("crates/tellit-ui/src/api.rs"),
        ];
        let tree = build_file_tree(&files);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name, "crates/tellit-ui/src");
        assert_eq!(tree.children[0].path, "crates/tellit-ui/src");
        assert_eq!(tree.children[0].children.len(), 2);

        let mut reviewed = HashSet::new();
        reviewed.insert(String::from("crates/tellit-ui/src/lib.rs"));
        assert!(get_reviewed_directory_paths(&tree, &reviewed).is_empty());

        reviewed.insert(String::from("crates/tellit-ui/src/api.rs"));
        assert!(get_reviewed_directory_paths(&tree, &reviewed).contains("crates/tellit-ui/src"));
    }

    #[test]
    fn filter_file_tree_retains_matching_paths_only() {
        let files = vec![
            sample_file("src/server/watch.rs"),
            sample_file("crates/tellit-ui/src/lib.rs"),
        ];
        let tree = build_file_tree(&files);
        let filtered = filter_file_tree(&tree, "watch");
        assert_eq!(filtered.children.len(), 1);
        assert_eq!(filtered.children[0].path, "src/server");
    }
}
