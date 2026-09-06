use super::*;
use ratatui::{backend::TestBackend, Terminal};

fn summary(number: u64) -> Summary {
    Summary {
        key: ItemKey {
            repository: GithubRepository::parse("example/project").expect("valid repository"),
            number,
            kind: ItemKind::PullRequest,
        },
        title: format!("Pull request {number}"),
        url: format!("https://github.com/example/project/pull/{number}"),
        state: "OPEN".into(),
        author: Some("alice".into()),
        updated_at: "2026-09-01T12:00:00Z".into(),
        created_at: "2026-09-01T12:00:00Z".into(),
        is_draft: false,
    }
}
fn details(number: u64) -> Box<ItemDetails> {
    Box::new(ItemDetails {
        summary: summary(number),
        body: Some(format!("Body for pull request {number}")),
        labels: Vec::new(),
        assignees: Vec::new(),
        comments: Vec::new(),
        review_comments: Vec::new(),
        reviews: Vec::new(),
        checks: Vec::new(),
        merge: Some(MergeState {
            node_id: "PR_example".into(),
            head_sha: "a".repeat(40),
            head_branch: "feature".into(),
            base_branch: "main".into(),
            mergeable: "MERGEABLE".into(),
            merge_state_status: "CLEAN".into(),
            review_decision: Some("APPROVED".into()),
            auto_merge_enabled: false,
            queue_enabled: false,
            viewer_can_update: true,
        }),
        additions: Some(1),
        deletions: Some(1),
        changed_files: Some(1),
        locked: false,
    })
}
fn queued_screen() -> GithubScreen {
    let mut screen = GithubScreen::new(
        ResolvedGithubScope {
            repositories: vec![
                GithubRepository::parse("example/project").expect("valid repository")
            ],
            organization: None,
        },
        "Example".into(),
    );
    screen.compute(Rect::new(0, 0, 120, 32));
    screen.activate(GithubAction::Tab(GithubTab::PullRequests));
    for (index, request) in screen.drain_requests().into_iter().enumerate() {
        let id = index as u64 + 1;
        screen.track_request(id);
        match request {
            GithubRequest::Viewer => screen.apply(
                id,
                Ok(GithubResponse::Viewer(Viewer {
                    login: "alice".into(),
                })),
            ),
            GithubRequest::Queue(_) => screen.apply(
                id,
                Ok(GithubResponse::Queue(Page {
                    items: vec![summary(1), summary(2)],
                    next_cursor: None,
                })),
            ),
            _ => panic!("unexpected initial request"),
        }
    }
    screen
}
fn loaded_screen() -> GithubScreen {
    let mut screen = queued_screen();
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let requests = screen.drain_requests();
    for (index, request) in requests.into_iter().enumerate() {
        let id = 10 + index as u64;
        screen.track_request(id);
        match request {
            GithubRequest::Details(item) => {
                screen.apply(id, Ok(GithubResponse::Details(details(item.number))))
            }
            _ => panic!("unexpected detail request"),
        }
    }
    screen
}
fn render_text(screen: &mut GithubScreen, width: u16, height: u16) -> String {
    screen.compute(Rect::new(0, 0, width, height));
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            crate::ui::github::render(screen, &crate::app::state::Palette::catppuccin(), frame)
        })
        .expect("draw GitHub screen");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn late_detail_response_cannot_replace_a_new_selection() {
    let mut screen = queued_screen();
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for request in screen.drain_requests() {
        assert!(matches!(request, GithubRequest::Details(_)));
        screen.track_request(10);
    }
    screen.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for request in screen.drain_requests() {
        assert!(matches!(request, GithubRequest::Details(_)));
        screen.track_request(20);
    }
    screen.apply(20, Ok(GithubResponse::Details(details(2))));
    screen.apply(10, Ok(GithubResponse::Details(details(1))));
    let text = render_text(&mut screen, 100, 30);
    assert!(text.contains("Body for pull request 2"));
    assert!(!text.contains("Body for pull request 1"));
}

#[test]
fn multiline_comment_keeps_its_draft_after_failure_and_blocks_duplicate_submit() {
    let mut screen = loaded_screen();
    let button = screen
        .geometry
        .controls
        .iter()
        .find(|control| control.action == GithubAction::Comment)
        .expect("visible comment button")
        .area;
    screen.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: button.x,
        row: button.y,
        modifiers: KeyModifiers::NONE,
    });
    screen.paste("first line\nsecond line");
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    screen.paste("third line");
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let requests = screen.drain_requests();
    assert!(
        matches!(&requests[..], [GithubRequest::Mutate(GithubMutation::Comment { body, .. })] if body == "first line\nsecond line\nthird line")
    );
    screen.track_request(30);
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        screen.drain_requests().is_empty(),
        "a second submit must not duplicate the comment"
    );
    screen.apply(30, Err("rate limited; retry later".into()));
    let text = render_text(&mut screen, 80, 24);
    assert!(text.contains("rate limited"));
    assert!(text.contains("first line"));
    assert!(text.contains("third line"));
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(&screen.drain_requests()[..], [GithubRequest::Mutate(GithubMutation::Comment { body, .. })] if body == "first line\nsecond line\nthird line")
    );
}

#[test]
fn compact_diff_keeps_content_and_all_actions_reachable() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Detail(DetailTab::Diff));
    for request in screen.drain_requests() {
        assert!(matches!(request, GithubRequest::Diff { .. }));
        screen.track_request(40);
    }
    screen.apply(
        40,
        Ok(GithubResponse::Diff(vec![super::super::diff::DiffFile {
            path: "src/lib.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some("@@ -1 +1 @@\n-old value\n+new value".into()),
        }])),
    );
    screen.activate(GithubAction::ToggleFiles);
    let text = render_text(&mut screen, 40, 10);
    assert!(text.contains("old value"));
    assert!(text.contains("new value"));
    let button = screen
        .geometry
        .controls
        .iter()
        .find(|control| control.action == GithubAction::Palette)
        .expect("visible actions button")
        .area;
    let effects = screen.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: button.x,
        row: button.y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(matches!(&effects[..], [GithubEffect::OpenPalette]));
    assert!(screen
        .contextual_actions()
        .iter()
        .any(|(action, _)| *action == GithubAction::RequestChanges));
}

#[test]
fn cross_side_diff_extension_never_submits_a_cross_side_comment() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Detail(DetailTab::Diff));
    for _request in screen.drain_requests() {
        screen.track_request(40);
    }
    screen.apply(
        40,
        Ok(GithubResponse::Diff(vec![super::super::diff::DiffFile {
            path: "src/lib.rs".into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some("@@ -1 +1 @@\n-old value\n+new value".into()),
        }])),
    );
    screen.activate(GithubAction::ToggleSplit);
    screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
    screen.activate(GithubAction::InlineComment);
    screen.paste("Keep the previous behavior.");
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(&screen.drain_requests()[..], [GithubRequest::Mutate(GithubMutation::InlineComment { comment, .. })] if comment.side == Side::Left && comment.start.is_none() && comment.line == 1)
    );
}

#[test]
fn scoped_runs_stage_large_repository_sets_without_dropping_repositories() {
    let repositories: Vec<_> = (0..70)
        .map(|index| {
            GithubRepository::parse(&format!("example/project-{index}")).expect("valid repository")
        })
        .collect();
    let mut screen = GithubScreen::new(
        ResolvedGithubScope {
            repositories: repositories.clone(),
            organization: None,
        },
        "Example".into(),
    );
    screen.activate(GithubAction::Tab(GithubTab::Actions));
    let mut seen = std::collections::BTreeSet::new();
    let mut next_id = 1;
    loop {
        let requests = screen.drain_requests();
        if requests.is_empty() {
            break;
        }
        assert!(
            requests.len() <= 4,
            "repository fan-out must stay below the backend queue capacity"
        );
        let batch: Vec<_> = requests
            .into_iter()
            .map(|request| {
                let id = next_id;
                next_id += 1;
                screen.track_request(id);
                (id, request)
            })
            .collect();
        for (id, request) in batch {
            let response = match request {
                GithubRequest::Viewer => GithubResponse::Viewer(Viewer {
                    login: "alice".into(),
                }),
                GithubRequest::Runs { repository, .. } => {
                    assert!(
                        seen.insert(repository),
                        "a repository's first page should only load once"
                    );
                    GithubResponse::Runs(Page {
                        items: Vec::new(),
                        next_cursor: None,
                    })
                }
                _ => panic!("unexpected scoped Actions request"),
            };
            screen.apply(id, Ok(response));
        }
    }
    assert_eq!(seen, repositories.into_iter().collect());
}

#[test]
fn explicit_refresh_loads_diff_against_the_new_head() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Detail(DetailTab::Diff));
    for _request in screen.drain_requests() {
        screen.track_request(40);
    }
    screen.activate(GithubAction::Refresh);
    assert!(screen.take_force_refresh());
    for request in screen.drain_requests() {
        assert!(
            matches!(request, GithubRequest::Details(_)),
            "refresh must load the head before the diff"
        );
        screen.track_request(50);
    }
    let mut changed = details(1);
    changed
        .merge
        .as_mut()
        .expect("pull request merge state")
        .head_sha = "b".repeat(40);
    screen.apply(50, Ok(GithubResponse::Details(changed)));
    assert!(
        screen.take_force_refresh(),
        "the dependent diff must also bypass the cache"
    );
    assert!(
        matches!(&screen.drain_requests()[..], [GithubRequest::Diff { head_sha, .. }] if head_sha == &"b".repeat(40))
    );
}

#[test]
fn deleting_a_comment_requires_ownership_and_explicit_confirmation() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Refresh);
    for _request in screen.drain_requests() {
        screen.track_request(50);
    }
    let mut item = details(1);
    item.comments = [("alice", 1, "Alice's comment"), ("bob", 2, "Bob's comment")]
        .into_iter()
        .map(|(author, id, body)| Comment {
            id,
            body: body.into(),
            html_url: format!("https://github.com/example/project/pull/1#issuecomment-{id}"),
            created_at: "2026-09-01T12:00:00Z".into(),
            updated_at: "2026-09-01T12:00:00Z".into(),
            user: Some(Viewer {
                login: author.into(),
            }),
            path: None,
            line: None,
            original_line: None,
            side: None,
            start_line: None,
            start_side: None,
            in_reply_to_id: None,
            commit_id: None,
            original_commit_id: None,
        })
        .collect();
    screen.apply(50, Ok(GithubResponse::Details(item)));
    screen.activate(GithubAction::Detail(DetailTab::Conversation));
    let text = render_text(&mut screen, 100, 30);
    let bob_row = text
        .lines()
        .position(|line| line.contains("Bob's comment"))
        .expect("Bob's comment is visible") as u16;
    screen.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: screen.geometry.detail.x,
        row: bob_row,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!screen
        .contextual_actions()
        .iter()
        .any(|(action, _)| matches!(
            action,
            GithubAction::EditComment | GithubAction::DeleteComment
        )));
    let text = render_text(&mut screen, 100, 30);
    let alice_row = text
        .lines()
        .position(|line| line.contains("Alice's comment"))
        .expect("Alice's comment is visible") as u16;
    screen.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: screen.geometry.detail.x,
        row: alice_row,
        modifiers: KeyModifiers::NONE,
    });
    screen.activate(GithubAction::DeleteComment);
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        screen.drain_requests().is_empty(),
        "confirmation defaults to Cancel"
    );
    screen.activate(GithubAction::DeleteComment);
    screen.activate(GithubAction::Submit);
    assert!(matches!(
        &screen.drain_requests()[..],
        [GithubRequest::Mutate(GithubMutation::DeleteComment {
            comment_id: 1,
            ..
        })]
    ));
}

#[test]
fn issue_navigation_drops_pull_request_only_queue_and_detail_tabs() {
    for tab in [DetailTab::Diff, DetailTab::Checks] {
        let mut screen = loaded_screen();
        screen.activate(GithubAction::Detail(tab));
        screen.activate(GithubAction::Tab(GithubTab::Issues));
        let mut issue = details(19);
        issue.summary.key.kind = ItemKind::Issue;
        issue.merge = None;
        for request in screen.drain_requests() {
            if let GithubRequest::Queue(queue) = request {
                assert_ne!(queue.queue, Queue::ReviewRequested);
                screen.track_request(60);
                screen.apply(
                    60,
                    Ok(GithubResponse::Queue(Page {
                        items: vec![issue.summary.clone()],
                        next_cursor: None,
                    })),
                );
            }
        }
        screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for request in screen.drain_requests() {
            if matches!(request, GithubRequest::Details(_)) {
                screen.track_request(61);
                screen.apply(61, Ok(GithubResponse::Details(issue.clone())));
            }
        }
        let text = render_text(&mut screen, 100, 30);
        assert!(text.contains("Body for pull request 19"), "{text}");
    }
}

#[test]
fn wrapped_diff_keyboard_navigation_advances_to_the_next_source_line() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Detail(DetailTab::Diff));
    for _ in screen.drain_requests() {
        screen.track_request(70);
    }
    screen.apply(
        70,
        Ok(GithubResponse::Diff(vec![super::super::diff::DiffFile {
            path: "src/lib.rs".into(),
            previous_path: None,
            status: "added".into(),
            patch: Some(format!(
                "@@ -0,0 +1,2 @@\n+{}\n+next line",
                "long source ".repeat(30)
            )),
        }])),
    );
    screen.activate(GithubAction::ToggleWrap);
    screen.compute(Rect::new(0, 0, 80, 30));
    screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    screen.activate(GithubAction::InlineComment);
    screen.paste("Comment on next line");
    screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(&screen.drain_requests()[..],
        [GithubRequest::Mutate(GithubMutation::InlineComment { comment, .. })]
        if comment.line == 2 && comment.side == Side::Right));
}

#[test]
fn switching_to_issues_uses_a_supported_queue() {
    let mut screen = loaded_screen();
    screen.activate(GithubAction::Queue(Queue::ReviewRequested));
    screen.activate(GithubAction::Tab(GithubTab::Issues));
    let requests = screen.drain_requests();
    assert!(requests.iter().any(|request| matches!(request,
        GithubRequest::Queue(queue) if queue.kind == ItemKind::Issue && queue.queue != Queue::ReviewRequested)));
    assert!(!requests.iter().any(|request| matches!(request,
        GithubRequest::Queue(queue) if queue.kind == ItemKind::Issue && queue.queue == Queue::ReviewRequested)));
}
