use std::path::{self, Path, PathBuf};

use call::ActiveCall;
use client::RECEIVE_TIMEOUT;
use collections::HashMap;
use git::{
    Oid,
    repository::{CommitData, GitCommitTemplate, Worktree as GitWorktree},
};
use gpui::{BackgroundExecutor, SharedString, TestAppContext};
use project::git_store::{CommitDataState, MAX_CONCURRENT_OBJECT_READS, Repository};
use serde_json::json;

use util::{path, rel_path::rel_path};

use crate::TestServer;

#[gpui::test]
async fn test_root_repo_common_dir_sync(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    // Set up a project whose root IS a git repository.
    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;
    executor.run_until_parked();

    // Host should see root_repo_common_dir pointing to .git at the root.
    let host_common_dir = project_a.read_with(cx_a, |project, cx| {
        let worktree = project.worktrees(cx).next().unwrap();
        worktree.read(cx).snapshot().root_repo_common_dir().cloned()
    });
    assert_eq!(
        host_common_dir.as_deref(),
        Some(path::Path::new(path!("/project/.git"))),
    );

    // Share the project and have client B join.
    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;
    executor.run_until_parked();

    // Guest should see the same root_repo_common_dir as the host.
    let guest_common_dir = project_b.read_with(cx_b, |project, cx| {
        let worktree = project.worktrees(cx).next().unwrap();
        worktree.read(cx).snapshot().root_repo_common_dir().cloned()
    });
    assert_eq!(
        guest_common_dir, host_common_dir,
        "guest should see the same root_repo_common_dir as host",
    );
}

async fn load_commit_data_batch(
    repository: &gpui::Entity<Repository>,
    shas: &[Oid],
    executor: &BackgroundExecutor,
    cx: &mut TestAppContext,
) -> HashMap<Oid, CommitData> {
    let states = cx.update(|cx| {
        shas.iter()
            .map(|sha| {
                (
                    *sha,
                    repository.update(cx, |repository, cx| {
                        repository.fetch_commit_data(*sha, true, cx).clone()
                    }),
                )
            })
            .collect::<Vec<_>>()
    });

    executor.run_until_parked();

    let mut commit_data = HashMap::default();
    for (sha, state) in states {
        let data = match state {
            CommitDataState::Loaded(data) => data.as_ref().clone(),
            CommitDataState::Loading(Some(shared)) => shared.await.unwrap().as_ref().clone(),
            CommitDataState::Loading(None) => {
                panic!("fetch_commit_data(..., true) should return an await-result state")
            }
        };
        commit_data.insert(sha, data);
    }

    commit_data
}

fn branch_list_snapshot(
    project: &gpui::Entity<project::Project>,
    cx: &mut TestAppContext,
) -> (Option<String>, Vec<String>) {
    project.read_with(cx, |project, cx| {
        let repos = project.repositories(cx);
        assert_eq!(repos.len(), 1, "project should have exactly 1 repository");
        let repo = repos.values().next().unwrap();
        let snapshot = repo.read(cx).snapshot();
        (
            snapshot
                .branch
                .as_ref()
                .map(|branch| branch.name().to_string()),
            snapshot
                .branch_list
                .iter()
                .map(|branch| branch.ref_name.to_string())
                .collect(),
        )
    })
}

fn assert_remote_cache_matches_local_cache(
    local_repository: &gpui::Entity<Repository>,
    remote_repository: &gpui::Entity<Repository>,
    cx_local: &mut TestAppContext,
    cx_remote: &mut TestAppContext,
) {
    let local_cache = cx_local.update(|cx| {
        local_repository.update(cx, |repository, _| repository.loaded_commit_data_for_test())
    });
    let remote_cache = cx_remote.update(|cx| {
        remote_repository.update(cx, |repository, _| repository.loaded_commit_data_for_test())
    });

    for (sha, remote_commit_data) in &remote_cache {
        let local_commit_data = local_cache
            .get(sha)
            .unwrap_or_else(|| panic!("local cache missing commit data for {sha}"));
        assert_eq!(
            local_commit_data.sha, remote_commit_data.sha,
            "local and remote cache should agree on sha for {sha}"
        );
        assert_eq!(
            local_commit_data.parents, remote_commit_data.parents,
            "local and remote cache should agree on parents for {sha}"
        );
        assert_eq!(
            local_commit_data.author_name, remote_commit_data.author_name,
            "local and remote cache should agree on author_name for {sha}"
        );
        assert_eq!(
            local_commit_data.author_email, remote_commit_data.author_email,
            "local and remote cache should agree on author_email for {sha}"
        );
        assert_eq!(
            local_commit_data.commit_timestamp, remote_commit_data.commit_timestamp,
            "local and remote cache should agree on commit_timestamp for {sha}"
        );
        assert_eq!(
            local_commit_data.subject, remote_commit_data.subject,
            "local and remote cache should agree on subject for {sha}"
        );
        assert_eq!(
            local_commit_data.message, remote_commit_data.message,
            "local and remote cache should agree on message for {sha}"
        );
    }
}

#[gpui::test]
async fn test_blob_read_rpcs_are_bounded_on_host(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
    cx_c: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    let client_c = server.create_client(cx_c, "user_c").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b), (&client_c, cx_c)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    const FILE_COUNT: usize = MAX_CONCURRENT_OBJECT_READS + 4;
    let names = (0..FILE_COUNT)
        .map(|index| format!("f{index:02}.txt"))
        .collect::<Vec<_>>();

    let mut tree = serde_json::Map::new();
    tree.insert(".git".to_owned(), json!({}));
    for (index, name) in names.iter().enumerate() {
        tree.insert(name.clone(), json!(format!("new-{index:02}\n")));
    }
    client_a
        .fs()
        .insert_tree(path!("/dir"), serde_json::Value::Object(tree))
        .await;

    let merge_base = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), format!("old-{index:02}\n")))
        .collect::<Vec<_>>();
    let dot_git = Path::new(path!("/dir/.git"));
    let oids = client_a
        .fs()
        .set_merge_base_content_for_repo(dot_git, &merge_base);
    let gate = client_a.fs().install_blob_read_gate_for_repo(dot_git);

    let (project_local, worktree_id) = client_a.build_local_project(path!("/dir"), cx_a).await;
    let project_id = active_call_a
        .update(cx_a, |call, cx| {
            call.share_project(project_local.clone(), cx)
        })
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;
    let project_c = client_c.join_remote_project(project_id, cx_c).await;
    executor.run_until_parked();

    // Two guests exceed the cap combined; only the host's own limiter holds them to it.
    let mut diffs = Vec::with_capacity(names.len() * 2);
    for (project, cx) in [(&project_b, &mut *cx_b), (&project_c, &mut *cx_c)] {
        let git_store = project.read_with(cx, |project, _| project.git_store().clone());
        let repo = project.read_with(cx, |project, cx| project.active_repository(cx).unwrap());
        for (index, name) in names.iter().enumerate() {
            let buffer = project
                .update(cx, |project, cx| {
                    project.open_buffer((worktree_id, rel_path(name)), cx)
                })
                .await
                .unwrap();
            diffs.push(git_store.update(cx, |git_store, cx| {
                git_store.open_diff_since(Some(oids[index]), buffer, repo.clone(), cx)
            }));
        }
    }
    executor.run_until_parked();

    assert_eq!(gate.peak_concurrent(), MAX_CONCURRENT_OBJECT_READS);

    gate.open();
    executor.run_until_parked();
    for diff in diffs {
        diff.await.unwrap();
    }
}

#[gpui::test]
async fn test_show_rpcs_are_routed_by_revision_kind_on_host(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    use futures::FutureExt as _;

    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(path!("/dir"), json!({ ".git": {} }))
        .await;
    let head_sha = "1".repeat(40);
    client_a
        .fs()
        .set_head_for_repo(Path::new(path!("/dir/.git")), &[], head_sha.clone());

    let (project_a, _) = client_a.build_local_project(path!("/dir"), cx_a).await;
    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;
    executor.run_until_parked();

    let repo_a = project_a.read_with(cx_a, |project, cx| project.active_repository(cx).unwrap());
    let repo_b = project_b.read_with(cx_b, |project, cx| project.active_repository(cx).unwrap());

    let (release_tx, release_rx) = futures::channel::oneshot::channel::<()>();
    let held = repo_a.update(cx_a, |repo, _| {
        repo.send_job("hold", None, move |_, _| async move {
            release_rx.await.ok();
        })
    });

    let mut by_sha = repo_b.update(cx_b, |repo, cx| repo.show_commit(head_sha.clone(), cx));
    let mut by_ref = repo_b.update(cx_b, |repo, _| repo.show("HEAD".into()));
    executor.run_until_parked();
    let details = (&mut by_sha)
        .now_or_never()
        .expect("show_commit waited on the host job queue")
        .unwrap();
    assert_eq!(details.sha.as_ref(), head_sha);
    assert!(
        (&mut by_ref).now_or_never().is_none(),
        "show by ref skipped the host job queue"
    );

    release_tx.send(()).ok();
    held.await.unwrap();
    executor.run_until_parked();
    assert_eq!(by_ref.await.unwrap().unwrap().sha.as_ref(), head_sha);
}

#[gpui::test]
async fn test_remote_git_worktrees(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;

    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;

    executor.run_until_parked();

    let repo_b = cx_b.update(|cx| project_b.read(cx).active_repository(cx).unwrap());

    // Initially only the main worktree (the repo itself) should be present
    let worktrees = cx_b
        .update(|cx| repo_b.update(cx, |repository, _| repository.worktrees()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].path, PathBuf::from(path!("/project")));

    // Client B creates a git worktree via the remote project
    let worktree_directory = PathBuf::from(path!("/project"));
    cx_b.update(|cx| {
        repo_b.update(cx, |repository, _| {
            repository.create_worktree(
                git::repository::CreateWorktreeTarget::NewBranch {
                    branch_name: "feature-branch".to_string(),
                    base_sha: Some("abc123".to_string()),
                },
                worktree_directory.join("feature-branch"),
            )
        })
    })
    .await
    .unwrap()
    .unwrap();

    executor.run_until_parked();

    // Client B lists worktrees — should see main + the one just created
    let worktrees = cx_b
        .update(|cx| repo_b.update(cx, |repository, _| repository.worktrees()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].path, PathBuf::from(path!("/project")));
    assert_eq!(worktrees[1].path, worktree_directory.join("feature-branch"));
    assert_eq!(
        worktrees[1].ref_name,
        Some("refs/heads/feature-branch".into())
    );
    assert_eq!(worktrees[1].sha.as_ref(), "abc123");

    // Verify from the host side that the worktree was actually created
    let host_worktrees = {
        let repo_a = cx_a.update(|cx| {
            project_a
                .read(cx)
                .repositories(cx)
                .values()
                .next()
                .unwrap()
                .clone()
        });
        cx_a.update(|cx| repo_a.update(cx, |repository, _| repository.worktrees()))
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(host_worktrees.len(), 2);
    assert_eq!(host_worktrees[0].path, PathBuf::from(path!("/project")));
    assert_eq!(
        host_worktrees[1].path,
        worktree_directory.join("feature-branch")
    );

    // Client B creates a second git worktree without an explicit commit
    cx_b.update(|cx| {
        repo_b.update(cx, |repository, _| {
            repository.create_worktree(
                git::repository::CreateWorktreeTarget::NewBranch {
                    branch_name: "bugfix-branch".to_string(),
                    base_sha: None,
                },
                worktree_directory.join("bugfix-branch"),
            )
        })
    })
    .await
    .unwrap()
    .unwrap();

    executor.run_until_parked();

    // Client B lists worktrees — should now have main + two created
    let worktrees = cx_b
        .update(|cx| repo_b.update(cx, |repository, _| repository.worktrees()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(worktrees.len(), 3);

    let feature_worktree = worktrees
        .iter()
        .find(|worktree| worktree.ref_name == Some("refs/heads/feature-branch".into()))
        .expect("should find feature-branch worktree");
    assert_eq!(
        feature_worktree.path,
        worktree_directory.join("feature-branch")
    );

    let bugfix_worktree = worktrees
        .iter()
        .find(|worktree| worktree.ref_name == Some("refs/heads/bugfix-branch".into()))
        .expect("should find bugfix-branch worktree");
    assert_eq!(
        bugfix_worktree.path,
        worktree_directory.join("bugfix-branch")
    );
    assert_eq!(bugfix_worktree.sha.as_ref(), "fake-sha");

    // Client B (guest) attempts to rename a worktree. This should fail
    // because worktree renaming is not forwarded through collab
    let rename_result = cx_b
        .update(|cx| {
            repo_b.update(cx, |repository, _| {
                repository.rename_worktree(
                    worktree_directory.join("feature-branch"),
                    worktree_directory.join("renamed-branch"),
                )
            })
        })
        .await
        .unwrap();
    assert!(
        rename_result.is_err(),
        "Guest should not be able to rename worktrees via collab"
    );

    executor.run_until_parked();

    // Verify worktrees are unchanged — still 3
    let worktrees = cx_b
        .update(|cx| repo_b.update(cx, |repository, _| repository.worktrees()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        worktrees.len(),
        3,
        "Worktree count should be unchanged after failed rename"
    );

    // Client B (guest) attempts to remove a worktree. This should fail
    // because worktree removal is not forwarded through collab
    let remove_result = cx_b
        .update(|cx| {
            repo_b.update(cx, |repository, _| {
                repository.remove_worktree(worktree_directory.join("feature-branch"), false)
            })
        })
        .await
        .unwrap();
    assert!(
        remove_result.is_err(),
        "Guest should not be able to remove worktrees via collab"
    );

    executor.run_until_parked();

    // Verify worktrees are unchanged — still 3
    let worktrees = cx_b
        .update(|cx| repo_b.update(cx, |repository, _| repository.worktrees()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        worktrees.len(),
        3,
        "Worktree count should be unchanged after failed removal"
    );
}

#[gpui::test]
async fn test_remote_git_head_sha(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;
    let local_head_sha = cx_a.update(|cx| {
        project_a
            .read(cx)
            .active_repository(cx)
            .unwrap()
            .update(cx, |repository, _| repository.head_sha())
    });
    let local_head_sha = local_head_sha.await.unwrap().unwrap();

    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;

    executor.run_until_parked();

    let remote_head_sha = cx_b.update(|cx| {
        project_b
            .read(cx)
            .active_repository(cx)
            .unwrap()
            .update(cx, |repository, _| repository.head_sha())
    });
    let remote_head_sha = remote_head_sha.await.unwrap();

    assert_eq!(remote_head_sha.unwrap(), local_head_sha);
}

#[gpui::test]
async fn test_remote_git_commit_data_batches(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;

    let commit_shas = [
        "0123456789abcdef0123456789abcdef01234567"
            .parse::<Oid>()
            .unwrap(),
        "1111111111111111111111111111111111111111"
            .parse::<Oid>()
            .unwrap(),
        "2222222222222222222222222222222222222222"
            .parse::<Oid>()
            .unwrap(),
        "3333333333333333333333333333333333333333"
            .parse::<Oid>()
            .unwrap(),
    ];

    client_a.fs().set_commit_data(
        Path::new(path!("/project/.git")),
        commit_shas.iter().enumerate().map(|(index, sha)| {
            (
                CommitData {
                    sha: *sha,
                    parents: Default::default(),
                    author_name: SharedString::from(format!("Author {index}")),
                    author_email: SharedString::from(format!("author{index}@example.com")),
                    commit_timestamp: 1_700_000_000 + index as i64,
                    subject: SharedString::from(format!("Subject {index}")),
                    message: SharedString::from(format!("Subject {index}\n\nBody {index}")),
                },
                false,
            )
        }),
    );

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;
    executor.run_until_parked();

    let repo_a = cx_a.update(|cx| project_a.read(cx).active_repository(cx).unwrap());

    let primed_before = load_commit_data_batch(&repo_a, &commit_shas[..2], &executor, cx_a).await;
    assert_eq!(
        primed_before.len(),
        2,
        "host should prime two commits before sharing"
    );

    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;

    executor.run_until_parked();

    let repo_b = cx_b.update(|cx| project_b.read(cx).active_repository(cx).unwrap());

    let remote_batch_one =
        load_commit_data_batch(&repo_b, &commit_shas[..3], &executor, cx_b).await;
    assert_eq!(remote_batch_one.len(), 3);
    for (index, sha) in commit_shas[..3].iter().enumerate() {
        let commit_data = remote_batch_one.get(sha).unwrap();
        assert_eq!(commit_data.sha, *sha);
        assert_eq!(commit_data.subject.as_ref(), format!("Subject {index}"));
        assert_eq!(
            commit_data.message.as_ref(),
            format!("Subject {index}\n\nBody {index}")
        );
    }

    let primed_after = load_commit_data_batch(&repo_a, &commit_shas[2..], &executor, cx_a).await;
    assert_eq!(
        primed_after.len(),
        2,
        "host should prime remaining commits after remote fetches"
    );

    let remote_batch_two =
        load_commit_data_batch(&repo_b, &commit_shas[1..], &executor, cx_b).await;
    assert_eq!(remote_batch_two.len(), 3);

    assert_remote_cache_matches_local_cache(&repo_a, &repo_b, cx_a, cx_b);
}

#[gpui::test]
async fn test_branch_list_sync(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;
    client_a.fs().insert_branches(
        Path::new(path!("/project/.git")),
        &["main", "feature-1", "feature-2"],
    );

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;
    executor.run_until_parked();

    let host_snapshot = branch_list_snapshot(&project_a, cx_a);
    assert_eq!(host_snapshot.0.as_deref(), Some("main"));
    assert_eq!(
        host_snapshot.1,
        vec![
            "refs/heads/feature-1".to_string(),
            "refs/heads/feature-2".to_string(),
            "refs/heads/main".to_string(),
        ]
    );

    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;

    executor.run_until_parked();

    let repo_b = cx_b.update(|cx| project_b.read(cx).active_repository(cx).unwrap());

    cx_b.update(|cx| {
        repo_b.update(cx, |repository, _cx| {
            repository.create_branch("totally-new-branch".to_string(), None)
        })
    })
    .await
    .unwrap()
    .unwrap();

    cx_b.update(|cx| {
        repo_b.update(cx, |repository, _cx| {
            repository.change_branch("totally-new-branch".to_string())
        })
    })
    .await
    .unwrap()
    .unwrap();

    executor.run_until_parked();

    let host_snapshot_after_update = branch_list_snapshot(&project_a, cx_a);
    assert_eq!(
        host_snapshot_after_update.0.as_deref(),
        Some("totally-new-branch")
    );
    assert_eq!(
        host_snapshot_after_update.1,
        vec![
            "refs/heads/feature-1".to_string(),
            "refs/heads/feature-2".to_string(),
            "refs/heads/main".to_string(),
            "refs/heads/totally-new-branch".to_string(),
        ]
    );

    let guest_snapshot_after_update = branch_list_snapshot(&project_b, cx_b);
    assert_eq!(guest_snapshot_after_update, host_snapshot_after_update);
}

#[gpui::test]
async fn test_linked_worktrees_sync(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
    cx_c: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    let client_c = server.create_client(cx_c, "user_c").await;
    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b), (&client_c, cx_c)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    // Set up a git repo with two linked worktrees already present.
    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({ ".git": {}, "file.txt": "content" }),
        )
        .await;

    let fs = client_a.fs();
    fs.add_linked_worktree_for_repo(
        Path::new(path!("/project/.git")),
        true,
        GitWorktree {
            path: PathBuf::from(path!("/worktrees/feature-branch")),
            ref_name: Some("refs/heads/feature-branch".into()),
            sha: "bbb222".into(),
            is_main: false,
            is_bare: false,
        },
    )
    .await;
    fs.add_linked_worktree_for_repo(
        Path::new(path!("/project/.git")),
        true,
        GitWorktree {
            path: PathBuf::from(path!("/worktrees/bugfix-branch")),
            ref_name: Some("refs/heads/bugfix-branch".into()),
            sha: "ccc333".into(),
            is_main: false,
            is_bare: false,
        },
    )
    .await;

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;

    // Wait for git scanning to complete on the host.
    executor.run_until_parked();

    // Verify the host sees 2 linked worktrees (main worktree is filtered out).
    let host_linked = project_a.read_with(cx_a, |project, cx| {
        let repos = project.repositories(cx);
        assert_eq!(repos.len(), 1, "host should have exactly 1 repository");
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        host_linked.len(),
        2,
        "host should have 2 linked worktrees (main filtered out)"
    );
    assert_eq!(
        host_linked[0].path,
        PathBuf::from(path!("/worktrees/bugfix-branch"))
    );
    assert_eq!(
        host_linked[0].ref_name,
        Some("refs/heads/bugfix-branch".into())
    );
    assert_eq!(host_linked[0].sha.as_ref(), "ccc333");
    assert_eq!(
        host_linked[1].path,
        PathBuf::from(path!("/worktrees/feature-branch"))
    );
    assert_eq!(
        host_linked[1].ref_name,
        Some("refs/heads/feature-branch".into())
    );
    assert_eq!(host_linked[1].sha.as_ref(), "bbb222");

    // Share the project and have client B join.
    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();
    let project_b = client_b.join_remote_project(project_id, cx_b).await;

    executor.run_until_parked();

    // Verify the guest sees the same linked worktrees as the host.
    let guest_linked = project_b.read_with(cx_b, |project, cx| {
        let repos = project.repositories(cx);
        assert_eq!(repos.len(), 1, "guest should have exactly 1 repository");
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        guest_linked, host_linked,
        "guest's linked_worktrees should match host's after initial sync"
    );

    // Now mutate: add a third linked worktree on the host side.
    client_a
        .fs()
        .add_linked_worktree_for_repo(
            Path::new(path!("/project/.git")),
            true,
            GitWorktree {
                path: PathBuf::from(path!("/worktrees/hotfix-branch")),
                ref_name: Some("refs/heads/hotfix-branch".into()),
                sha: "ddd444".into(),
                is_main: false,
                is_bare: false,
            },
        )
        .await;

    // Wait for the host to re-scan and propagate the update.
    executor.run_until_parked();

    // Verify host now sees 3 linked worktrees.
    let host_linked_updated = project_a.read_with(cx_a, |project, cx| {
        let repos = project.repositories(cx);
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        host_linked_updated.len(),
        3,
        "host should now have 3 linked worktrees"
    );
    assert_eq!(
        host_linked_updated[2].path,
        PathBuf::from(path!("/worktrees/hotfix-branch"))
    );

    // Verify the guest also received the update.
    let guest_linked_updated = project_b.read_with(cx_b, |project, cx| {
        let repos = project.repositories(cx);
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        guest_linked_updated, host_linked_updated,
        "guest's linked_worktrees should match host's after update"
    );

    // Now mutate: remove one linked worktree from the host side.
    client_a
        .fs()
        .remove_worktree_for_repo(
            Path::new(path!("/project/.git")),
            true,
            "refs/heads/bugfix-branch",
        )
        .await;

    executor.run_until_parked();

    // Verify host now sees 2 linked worktrees (feature-branch and hotfix-branch).
    let (host_linked_after_removal, host_git_paths_after_removal) =
        project_a.read_with(cx_a, |project, cx| {
            let repos = project.repositories(cx);
            let repo = repos.values().next().unwrap();
            let repo = repo.read(cx);
            (
                repo.linked_worktrees().to_vec(),
                (
                    repo.repository_dir_abs_path.to_path_buf(),
                    repo.common_dir_abs_path.to_path_buf(),
                ),
            )
        });
    assert_eq!(
        host_linked_after_removal.len(),
        2,
        "host should have 2 linked worktrees after removal"
    );
    assert!(
        host_linked_after_removal
            .iter()
            .all(|wt| wt.ref_name != Some("refs/heads/bugfix-branch".into())),
        "bugfix-branch should have been removed"
    );

    // Verify the guest also reflects the removal.
    let guest_linked_after_removal = project_b.read_with(cx_b, |project, cx| {
        let repos = project.repositories(cx);
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        guest_linked_after_removal, host_linked_after_removal,
        "guest's linked_worktrees should match host's after removal"
    );

    // Test DB roundtrip: client C joins late, getting state from the database.
    // This verifies that linked_worktrees are persisted and restored correctly.
    let project_c = client_c.join_remote_project(project_id, cx_c).await;
    executor.run_until_parked();

    let late_joiner_linked = project_c.read_with(cx_c, |project, cx| {
        let repos = project.repositories(cx);
        assert_eq!(
            repos.len(),
            1,
            "late joiner should have exactly 1 repository"
        );
        let repo = repos.values().next().unwrap();
        repo.read(cx).linked_worktrees().to_vec()
    });
    assert_eq!(
        late_joiner_linked, host_linked_after_removal,
        "late-joining client's linked_worktrees should match host's (DB roundtrip)"
    );
    let late_joiner_git_paths = project_c.read_with(cx_c, |project, cx| {
        let repos = project.repositories(cx);
        let repo = repos.values().next().unwrap();
        let repo = repo.read(cx);
        (
            repo.repository_dir_abs_path.to_path_buf(),
            repo.common_dir_abs_path.to_path_buf(),
        )
    });
    assert_eq!(
        late_joiner_git_paths, host_git_paths_after_removal,
        "late-joining client's git directory paths should match host's (DB roundtrip)"
    );

    // Test reconnection: disconnect client B (guest) and reconnect.
    // After rejoining, client B should get linked_worktrees back from the DB.
    server.disconnect_client(client_b.peer_id().unwrap());
    executor.advance_clock(RECEIVE_TIMEOUT);
    executor.run_until_parked();

    // Client B reconnects automatically.
    executor.advance_clock(RECEIVE_TIMEOUT);
    executor.run_until_parked();

    // Verify client B still has the correct linked worktrees after reconnection.
    let (guest_linked_after_reconnect, guest_git_paths_after_reconnect) =
        project_b.read_with(cx_b, |project, cx| {
            let repos = project.repositories(cx);
            assert_eq!(
                repos.len(),
                1,
                "guest should still have exactly 1 repository after reconnect"
            );
            let repo = repos.values().next().unwrap();
            let repo = repo.read(cx);
            (
                repo.linked_worktrees().to_vec(),
                (
                    repo.repository_dir_abs_path.to_path_buf(),
                    repo.common_dir_abs_path.to_path_buf(),
                ),
            )
        });
    assert_eq!(
        guest_linked_after_reconnect, host_linked_after_removal,
        "guest's linked_worktrees should survive guest disconnect/reconnect"
    );
    assert_eq!(
        guest_git_paths_after_reconnect, host_git_paths_after_removal,
        "guest's git directory paths should survive guest disconnect/reconnect"
    );
}

#[gpui::test]
async fn test_load_commit_template_over_collab(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;

    server
        .create_room(&mut [(&client_a, cx_a), (&client_b, cx_b)])
        .await;
    let active_call_a = cx_a.read(ActiveCall::global);

    client_a
        .fs()
        .insert_tree(
            path!("/project"),
            json!({
                ".git": {},
                "README.md": "# Project's README"
            }),
        )
        .await;

    client_a
        .fs()
        .with_git_state(Path::new(path!("/project/.git")), false, |state| {
            state.commit_template = Some(GitCommitTemplate {
                template: "feat: add awesome feature".to_string(),
            })
        })
        .expect("Should update git state");

    let (project_a, _) = client_a.build_local_project(path!("/project"), cx_a).await;
    let project_id = active_call_a
        .update(cx_a, |call, cx| call.share_project(project_a.clone(), cx))
        .await
        .unwrap();

    let project_b = client_b.join_remote_project(project_id, cx_b).await;
    executor.run_until_parked();

    let repository_b = project_b.update(cx_b, |project, cx| project.active_repository(cx).unwrap());
    let commit_template = repository_b
        .update(cx_b, |repository, _cx| {
            repository.load_commit_template_text()
        })
        .await
        .unwrap()
        .unwrap()
        .expect("Guest should be able to load the host's commit template");

    assert_eq!(commit_template.template, "feat: add awesome feature");
}
