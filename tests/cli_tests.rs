use std::fs;
use tempfile::tempdir;
use commenter::{
    get_change_commits, get_commit_threads, get_thread, reply_comment, resolve_comment, LineRange,
    ThreadData,
};

#[test]
fn test_full_comment_workflow() {
    let dir = tempdir().unwrap();
    let store = dir.path().join(".comments");
    let workspace = dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // We can simulate add by manually creating a thread file or using the API
    let change_id = "kkmpptxztestchange";
    let git_commit = "e9a03f48a127bc9102919d7bca89104812345678";

    let thread_dir = store.join(change_id).join(git_commit);
    fs::create_dir_all(&thread_dir).unwrap();

    let thread_id = "a1b2c3d4-e5f6-4a5b-8c9d-0e1f2a3b4c5d";
    let thread_file = thread_dir.join(format!("{}.json", thread_id));

    let data = ThreadData {
        filepath: "src/lib.rs".to_string(),
        line_range: Some(LineRange { start: 10, end: 25 }),
        comments: vec!["Consider refactoring this loop".to_string()],
        resolved: false,
    };
    fs::write(&thread_file, serde_json::to_vec_pretty(&data).unwrap()).unwrap();

    // Test get thread
    let info = get_thread(&store, thread_id).unwrap();
    assert_eq!(info.thread_id, thread_id);
    assert_eq!(info.change_id, change_id);
    assert_eq!(info.git_commit_hash, git_commit);
    assert_eq!(info.filepath, "src/lib.rs");
    assert_eq!(info.line_range, Some(LineRange { start: 10, end: 25 }));
    assert_eq!(info.comments.len(), 1);
    assert!(!info.resolved);

    // Test reply
    let replied_id = reply_comment(&store, "a1b2c3d4", "Done in the latest revision".to_string()).unwrap();
    assert_eq!(replied_id, thread_id);

    let info_after_reply = get_thread(&store, thread_id).unwrap();
    assert_eq!(info_after_reply.comments.len(), 2);
    assert_eq!(info_after_reply.comments[1], "Done in the latest revision");

    // Test resolve
    let (res_id, is_resolved) = resolve_comment(&store, "a1b2c3d4", false).unwrap();
    assert_eq!(res_id, thread_id);
    assert!(is_resolved);

    let info_after_resolve = get_thread(&store, thread_id).unwrap();
    assert!(info_after_resolve.resolved);

    // Test unresolve
    let (unres_id, is_resolved_again) = resolve_comment(&store, "a1b2c3d4", true).unwrap();
    assert_eq!(unres_id, thread_id);
    assert!(!is_resolved_again);

    let info_after_unresolve = get_thread(&store, thread_id).unwrap();
    assert!(!info_after_unresolve.resolved);

    // Test get change
    let commits = get_change_commits(&store, change_id).unwrap();
    assert_eq!(commits, vec![git_commit]);

    // Test get change prefix
    let commits_prefix = get_change_commits(&store, "kkmpptxz").unwrap();
    assert_eq!(commits_prefix, vec![git_commit]);

    // Test get commit
    let threads = get_commit_threads(&store, git_commit).unwrap();
    assert_eq!(threads, vec![thread_id]);

    // Test get commit prefix
    let threads_prefix = get_commit_threads(&store, "e9a03f48").unwrap();
    assert_eq!(threads_prefix, vec![thread_id]);
}
