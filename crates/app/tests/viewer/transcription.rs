//! Transcribing pages, the cache, models and Transcribe All.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::eval_log::compare;
use notebook_app::i18n::ocr_error;
use notebook_app::{Command, Event, Fetch, NotebookApp, OnTranscribeCompleted, Transcriber, t};
use notebook_core::cache::Cache;
use notebook_core::claude_transcribe::Fetched;
use notebook_core::llm_model::Model;
use notebook_core::markdown_style::MarkdownStyle;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use crate::common::*;

#[test]
fn transcribed_view_transcribes_once() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    click_command(&harness, Command::ViewTranscribed);
    step_until(&mut harness, "# Thurs 23 Oct");
    harness.get_by_label("~~tap~~");

    click_command(&harness, Command::ViewOriginal);
    settle(&mut harness);
    harness.get_by_label(&t!("current-page", name = "a.png"));
    click_command(&harness, Command::ViewTranscribed);
    settle(&mut harness);
    harness.get_by_label_contains("# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_transcription_can_be_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fetch: Fetch = Arc::new(
        move |_, _, _, _| match counter.fetch_add(1, Ordering::SeqCst) {
            0 => Err(overloaded()),
            _ => Ok(recorded()),
        },
    );
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo())]);

    click_command(&harness, Command::ViewTranscribed);
    let failed = t!(
        "transcribe-failed",
        name = "a.png",
        error = ocr_error(&overloaded())
    );
    step_until(&mut harness, &failed);
    // A failure isn't retried by itself.
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    harness.get_by_label(&t!("transcribe-retry")).click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(harness.query_by_label(&failed).is_none());
}

#[test]
fn selecting_another_page_asks_before_transcribing() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    harness.get_by_label("b.png").click();
    settle(&mut harness);
    harness.get_by_label(&t!("not-transcribed-yet", name = "b.png"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    click_command(&harness, Command::Transcribe);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn switching_to_transcribed_transcribes_the_shown_page() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    harness.get_by_label("b.png").click();
    click_command(&harness, Command::ViewCleaned);
    settle(&mut harness);

    click_command(&harness, Command::ViewTranscribed);
    step_until(&mut harness, "# Thurs 23 Oct");
    harness.get_by_label(&t!("current-page-cleaned", name = "b.png"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cached_pages_show_straight_away() {
    let dir = test_dir("show_straight_away");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    // Later, the same photo under another name, after an uncached one.
    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", png()), ("b.png", page_photo())]);
    show_transcript(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    harness.get_by_label("b.png").click();
    step_until(&mut harness, "# Thurs 23 Oct");
    assert!(
        harness
            .query_by_label(&Command::Transcribe.label())
            .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "b.png not sent");
    harness.get_by_label_contains(&before_mark(t!("transcript-from-cache", when = MARK)));
    harness.get_by_label_contains(&by("claude-opus-5-5"));
}

#[test]
fn cached_pages_load_without_a_transcriber() {
    let dir = test_dir("without_transcriber");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    let app = NotebookApp::with_transcription(None, Some(Cache::new(dir.path())));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    wait_for("the cached transcript", || {
        harness.step();
        !harness.state().eval_log().events().is_empty()
    });
    assert!(matches!(
        harness.state().eval_log().events(),
        [Event::Transcribed {
            from_cache: true,
            ..
        }]
    ));
}

/// Without credentials, cached transcripts can be viewed and exported, but nothing can be sent.
#[test]
fn cached_transcripts_without_a_transcriber() {
    let dir = test_dir("cached_without_transcriber");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    let app = NotebookApp::with_transcription(None, Some(Cache::new(dir.path())));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    for label in [
        &Command::ViewTranscribed.label(),
        &Command::ExportAs.label(),
    ] {
        assert!(!disabled(&harness, label), "{label} is disabled");
    }
    for label in [
        &Command::ForceRetranscribe.label(),
        &Command::TranscribeAll.label(),
    ] {
        assert!(disabled(&harness, label), "{label} is enabled");
    }
    assert!(export_to_clipboard(&mut harness).contains("# Thurs 23 Oct"));

    drop_files(&mut harness, &[("b.png", png())]);
    harness.get_by_label("b.png").click();
    step_until(&mut harness, &t!("not-cached", name = "b.png"));
    harness.get_by_label(&t!("no-transcriber-hint"));
    assert!(
        harness
            .query_by_label(&Command::Transcribe.label())
            .is_none()
    );
}

#[test]
fn force_retranscribes_without_asking_if_uncorrected() {
    let answers = vec![Ok(recorded()), Ok(recorded_fixture("fallback"))];
    let (fetch, calls) = answering(answers);
    let gate = Arc::new(RwLock::new(()));
    let dir = test_dir("force");
    let mut harness = cached_harness(gated_fetch(fetch, gate.clone()), dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    status_placement(
        &harness,
        &before_mark(t!("transcript-from-claude", when = MARK)),
    );

    let held = gate.write().unwrap();
    click_command(&harness, Command::ForceRetranscribe);
    harness.step();
    harness.step();
    // Progress replaces the old transcript meanwhile.
    harness.get_by_label(&t!("transcribing-page", name = "a.png"));
    assert!(harness.query_by_label("# Thurs 23 Oct").is_none());
    assert!(
        harness
            .query_by_label(&Command::ForceRetranscribe.label())
            .is_none()
    );

    drop(held);
    step_until(&mut harness, &by("claude-opus-4-8"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn the_selected_model_transcribes_and_is_compared_with_the_other() {
    // Answers as the model asked.
    let fetch: Fetch = Arc::new(|_, _, _, model| {
        let recorded = recorded();
        let body = String::from_utf8(recorded.body).unwrap().replace(
            "\"model\": \"claude-opus-5-5\"",
            &format!("\"model\": \"{}\"", model.id()),
        );
        Ok(Fetched {
            body: body.into_bytes(),
            ..recorded
        })
    });
    // Each page sent, by whom, and how it compares with other models' cached transcripts.
    let dir = test_dir("select_model");
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let on_transcribe_completed: OnTranscribeCompleted = {
        let (sent, cache) = (sent.clone(), Cache::new(dir.path()));
        Arc::new(
            move |page: &str, bytes: &[u8], t: &notebook_core::transcript::Transcript| {
                let compared = compare::with_cached(&cache, bytes, t);
                let compared: Vec<_> = compared.iter().map(|(m, c)| (*m, c.similarity)).collect();
                sent.lock()
                    .unwrap()
                    .push((page.to_owned(), t.model.clone(), compared));
            },
        )
    };
    let transcriber =
        Transcriber::new(fetch).on_transcribe_completed(on_transcribe_completed.clone());
    let mut harness = harness_with_cache(transcriber, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert!(
        harness.query_by_label(&t!("settings-model")).is_none(),
        "it's in Settings, not the toolbar"
    );

    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("model-sonnet")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);
    click_command(&harness, Command::ForceRetranscribe);
    step_until(&mut harness, &by("claude-sonnet-5-5"));
    let sent_so_far = std::mem::take(&mut *sent.lock().unwrap());
    let a = || "a.png".to_owned();
    assert_eq!(
        sent_so_far,
        [
            (a(), "claude-opus-5-5".to_owned(), vec![]),
            (
                a(),
                "claude-sonnet-5-5".to_owned(),
                vec![(Model::Opus, 1.0)]
            ),
        ]
    );
    let cache = Cache::new(dir.path());
    let photo = page_photo();
    let style = MarkdownStyle::default();
    assert!(cache.get(&photo, Model::Opus, &style).is_some());
    assert!(cache.get(&photo, Model::Sonnet, &style).is_some());

    // Either entry will do for a new page, which isn't sent.
    let (fetch, calls) = counting_fetch();
    let transcriber = Transcriber::new(fetch).on_transcribe_completed(on_transcribe_completed);
    let mut harness = harness_with_cache(transcriber, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sent.lock().unwrap().is_empty());
    assert_eq!(events(&harness), ["transcribed from_cache=true"]);
}

#[test]
fn failed_force_keeps_the_corrected_transcript_and_the_cache() {
    let dir = test_dir("failed_force");
    let answers = vec![Ok(recorded()), Err(overloaded())];
    let (fetch, _) = answering(answers);
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    correct_plumber(&mut harness);

    click_command(&harness, Command::ForceRetranscribe);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-force", name = "a.png"));
    harness.get_by_label(&t!("confirm-force-button")).click();
    let failed = before_mark(t!(
        "transcript-retranscribe-failed",
        error = ocr_error(&overloaded()),
        when = MARK
    ));
    step_until(&mut harness, &failed);
    status_placement(&harness, &failed);
    harness.get_by_label("plumbers");

    // The cached answer is still Claude's.
    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    harness.get_by_label("plumber");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn transcribe_all_starts_straight_away_for_a_few_pages() {
    let (fetch, calls) = counting_fetch();
    let mut harness = harness_with(Transcriber::new(fetch));
    drop_files(
        &mut harness,
        &[("a.png", png()), ("b.png", png()), ("c.png", png())],
    );

    click_command(&harness, Command::TranscribeAll);
    step_until_transcribed(&mut harness);
    let ask = t!("confirm-transcribe-all", count = 3);
    assert!(harness.query_by_label(&ask).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let label = Command::TranscribeAll.label();
    let all = harness.get_by_label(&label);
    assert!(all.accesskit_node().is_disabled(), "nothing left");
}

#[test]
fn transcribe_all_asks_first_and_sends_four_at_a_time() {
    let (calls, running, most) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let gate = Arc::new(RwLock::new(()));
    let fetch: Fetch = {
        let (calls, running, most, gate) =
            (calls.clone(), running.clone(), most.clone(), gate.clone());
        Arc::new(move |_, _, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            drop(gate.read().unwrap());
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(recorded())
        })
    };
    let mut harness = harness_with(Transcriber::new(fetch));
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let files: Vec<_> = names.iter().map(|n| (*n, png())).collect();
    drop_files(&mut harness, &files);

    click_command(&harness, Command::TranscribeAll);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-transcribe-all", count = 6));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    click_command(&harness, Command::TranscribeAll);
    settle(&mut harness);
    let held = gate.write().unwrap();
    harness
        .get_by_label(&t!("confirm-transcribe-all-button"))
        .click();
    wait_for("four pages to be sent", || {
        harness.step();
        running.load(Ordering::SeqCst) >= 4
    });
    // A few more frames, in which a fifth mustn't start.
    harness.run_steps(4);
    drop(held);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(most.load(Ordering::SeqCst), 4);
}

#[test]
fn transcribe_all_uses_the_cache() {
    let dir = test_dir("transcribe_all");
    let (fetch, _) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    let (fetch, calls) = counting_fetch();
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    click_command(&harness, Command::TranscribeAll);
    step_until_transcribed(&mut harness);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "only b.png sent");
}

#[test]
fn deleting_a_queued_page_means_it_isnt_sent() {
    let (fetch, calls) = counting_fetch();
    let gate = Arc::new(RwLock::new(()));
    let mut harness = harness_with(Transcriber::new(gated_fetch(fetch, gate.clone())));
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let files: Vec<_> = names.iter().map(|n| (*n, png())).collect();
    drop_files(&mut harness, &files);
    click_command(&harness, Command::TranscribeAll);
    settle(&mut harness);
    let held = gate.write().unwrap();
    harness
        .get_by_label(&t!("confirm-transcribe-all-button"))
        .click();
    harness.step();

    harness.get_by_label("f.png").click();
    harness.step();
    click_command(&harness, Command::Delete);
    harness.step();
    drop(held);
    step_until_transcribed(&mut harness);
    assert_eq!(thumbnails(&harness).len(), 5);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}

#[test]
fn dragging_the_split_widens_the_page() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let width = |harness: &Harness<'_, NotebookApp>| {
        let label = t!("current-page-cleaned", name = "a.png");
        let image = harness.get_by_label(&label);
        image.accesskit_node().bounding_box().unwrap().width()
    };
    let before = width(&harness);

    let start = centre(&harness, &t!("resize-transcript"));
    drag(&mut harness, start, egui::pos2(start.x + 60.0, start.y));
    settle(&mut harness);
    assert!(
        width(&harness) > before + 50.0,
        "{before} -> {}",
        width(&harness)
    );
}
