//! The Settings dialog, and other dialogs.

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use notebook_app::{Command, Fetch, NotebookApp, t};
use notebook_core::cache::Cache;
use notebook_core::claude_transcribe::Fetched;
use notebook_core::llm_model::Model;
use notebook_core::settings::Settings;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::*;

/// macOS order: the affirmative button rightmost, after Cancel.
#[test]
fn dialogs_put_the_affirmative_button_last() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    let left = |harness: &Harness<'_, NotebookApp>, label: &str| {
        let node = harness.get_by_role_and_label(Role::Button, label);
        node.accesskit_node().bounding_box().unwrap().x0
    };

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    let cancel = t!("dialog-cancel");
    assert!(left(&harness, &cancel) < left(&harness, &t!("export-go")));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    assert!(left(&harness, &cancel) < left(&harness, &t!("confirm-delete-button")));
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);

    press_command(&mut harness, egui::Key::Comma);
    assert!(left(&harness, &cancel) < left(&harness, &t!("settings-save")));
}

#[test]
fn settings_open_without_pages_and_cancelling_keeps_them() {
    let mut harness = harness();
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")));

    harness.get_by_label(&t!("model-sonnet")).click();
    press(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "closed"
    );
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")), "Escape kept Opus");

    harness.get_by_label(&t!("model-sonnet")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("dialog-cancel")).click();
    settle(&mut harness);
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-opus")), "Cancel kept Opus");
}

#[test]
fn saved_settings_are_kept_for_next_time() {
    let dir = test_dir("settings_file");
    let path = dir.path().join("settings.json");
    let app = || harness_for(NotebookApp::default().with_settings_file(path.clone()));

    let mut harness = app();
    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("model-sonnet")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "closed"
    );
    assert_eq!(Settings::load(&path).model, Model::Sonnet);

    let mut harness = app();
    press_command(&mut harness, egui::Key::Comma);
    assert!(checked(&harness, &t!("model-sonnet")));
}

/// A new markdown style reaches transcripts the user hasn't corrected, read again from the cache,
/// and corrected ones once they're reset; none is sent again.
#[test]
fn a_new_markdown_style_restyles_unedited_transcripts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let fetch: Fetch = {
        let calls = calls.clone();
        Arc::new(move |_, _, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let recorded = recorded();
            let body = String::from_utf8(recorded.body)
                .unwrap()
                .replace("buy eggs", "buy <u>eggs</u>");
            Ok(Fetched {
                body: body.into_bytes(),
                ..recorded
            })
        })
    };
    let dir = test_dir("restyle");
    let mut harness = cached_harness(fetch, dir.path());
    drop_files(
        &mut harness,
        &[("a.png", page_photo()), ("b.png", another_page_photo())],
    );
    show_transcript(&mut harness);
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    click_command(&harness, Command::Transcribe);
    step_until(&mut harness, "buy _eggs_");
    correct_plumber(&mut harness);

    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("settings-underlines")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("underline-asterisks")).click();
    settle(&mut harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);

    harness.get_by_label("a.png").click();
    step_until(&mut harness, "buy *eggs*");
    harness.get_by_label("b.png").click();
    settle(&mut harness);
    harness.get_by_label_contains("buy _eggs_");
    harness.get_by_label("plumbers");

    click_command(&harness, Command::ResetTranscript);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-reset-button")).click();
    step_until(&mut harness, "buy *eggs*");
    harness.get_by_label("plumber");

    assert_eq!(calls.load(Ordering::SeqCst), 2, "each page sent once");
    let transcribed = events(&harness)
        .into_iter()
        .filter(|e| e.starts_with("transcribed"))
        .collect::<Vec<_>>();
    assert_eq!(
        transcribed,
        [
            "transcribed from_cache=false",
            "transcribed from_cache=false"
        ]
    );
}

#[test]
fn enter_does_nothing_while_the_affirmative_button_is_disabled() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo()), ("b.png", png())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::ExportAs);
    settle(&mut harness);
    harness.get_by_label(&t!("export-not-transcribed", names = "b.png"));
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, &t!("export-go"));
    let copied = before_mark(t!("export-copied", count = MARK, name = MARK));
    assert!(harness.query_by_label_contains(&copied).is_none());
}

#[test]
fn enter_is_the_focused_widgets() {
    let mut harness = harness_with(transcriber());
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);

    click_command(&harness, Command::Delete);
    settle(&mut harness);
    harness.get_by_label(&t!("dialog-cancel")).focus();
    settle(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    let ask = t!("confirm-delete", name = "a.png");
    assert!(harness.query_by_label(&ask).is_none());
    assert_eq!(thumbnails(&harness), ["a.png"]);
}

/// Opens Settings on a harness whose cache, in `cache_dir`, has a.png's transcript, and whose
/// settings are saved in `cache_dir`'s parent.
fn cached_settings(dir: &std::path::Path) -> Harness<'static, NotebookApp> {
    let app = NotebookApp::with_transcription(Some(transcriber()), Some(Cache::new(cache(dir))))
        .with_settings_file(dir.join("settings.json"));
    let mut harness = harness_for(app);
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    press_command(&mut harness, egui::Key::Comma);
    harness
}

fn cache(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("cache")
}

fn cached_count(dir: &std::path::Path) -> usize {
    Cache::new(cache(dir)).count().unwrap()
}

#[test]
fn settings_show_where_the_cache_is_and_how_much_is_in_it() {
    let dir = test_dir("cache_location");
    let harness = cached_settings(dir.path());
    let location = t!("settings-cache-location");
    let shown = harness.get_by_label(&location).accesskit_node().value();
    let shown = shown.unwrap_or_default();
    assert_eq!(shown, cache(dir.path()).display().to_string());
    harness.get_by_label(&t!("settings-cache-count", count = 1));
    assert!(checked(&harness, &t!("settings-cache-enable")));
    assert!(!disabled(&harness, &t!("settings-cache-clear")));
}

/// Clear Cache is red, as it deletes: the same red as a dangerous dialog button.
#[test]
fn clear_cache_is_red() {
    let dir = test_dir("cache_red");
    let mut harness = cached_settings(dir.path());
    let red = fill_behind(&harness, &t!("settings-cache-clear"));
    assert_ne!(red, fill_behind(&harness, &t!("settings-save")));

    harness.get_by_label(&t!("settings-cache-clear")).click();
    settle(&mut harness);
    let clear = t!("settings-cache-clear");
    let buttons = harness.query_all_by_label(&clear).collect::<Vec<_>>();
    let question = buttons.last().unwrap().rect();
    assert_eq!(red, fill_at(&harness, question.center()));
}

/// The fill of the widget labelled `label`.
fn fill_behind(harness: &Harness<'_, NotebookApp>, label: &str) -> egui::Color32 {
    fill_at(harness, harness.get_by_label(label).rect().center())
}

/// The fill of the last rectangle painted under `at`.
fn fill_at(harness: &Harness<'_, NotebookApp>, at: egui::Pos2) -> egui::Color32 {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.rect.contains(at) => Some(rect.fill),
            _ => None,
        })
        .rfind(|fill| *fill != egui::Color32::TRANSPARENT)
        .unwrap_or_else(|| panic!("nothing painted at {at:?}"))
}

#[test]
fn clearing_the_cache_asks_first() {
    let dir = test_dir("clear_cache");
    let mut harness = cached_settings(dir.path());
    let ask = t!("confirm-clear-cache", count = 1);

    harness.get_by_label(&t!("settings-cache-clear")).click();
    settle(&mut harness);
    harness.get_by_label(&ask);
    // The question's, not the dialog's beneath it.
    let cancel = t!("dialog-cancel");
    harness.query_all_by_label(&cancel).last().unwrap().click();
    settle(&mut harness);
    assert!(harness.query_by_label(&ask).is_none(), "closed");
    assert_eq!(cached_count(dir.path()), 1, "Cancel kept it");

    harness.get_by_label(&t!("settings-cache-clear")).click();
    settle(&mut harness);
    // The question's button, not the dialog's beneath it.
    let clear = t!("settings-cache-clear");
    harness.query_all_by_label(&clear).last().unwrap().click();
    settle(&mut harness);
    assert!(harness.query_by_label(&ask).is_none(), "closed");
    assert_eq!(cached_count(dir.path()), 0);
    harness.get_by_label(&t!("settings-cache-count", count = 0));
    assert!(disabled(&harness, &t!("settings-cache-clear")));
    harness.get_by_label(&t!("settings-save")).click();
    settle(&mut harness);
    harness.get_by_label_contains("# Thurs 23 Oct");
}

/// Saves the settings with caching switched off.
fn switch_caching_off(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label(&t!("settings-cache-enable")).click();
    settle(harness);
    harness.get_by_label(&t!("settings-save")).click();
    settle(harness);
}

#[test]
fn switching_caching_off_offers_to_clear_it() {
    let dir = test_dir("cache_off");
    let mut harness = cached_settings(dir.path());
    switch_caching_off(&mut harness);
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "saved"
    );
    assert!(!Settings::load(&dir.path().join("settings.json")).cache_transcripts);

    harness.get_by_label(&t!("confirm-cache-off", count = 1));
    harness.get_by_role_and_label(Role::Button, &t!("confirm-cache-off-keep"));
    assert!(
        harness.query_by_label(&t!("dialog-cancel")).is_none(),
        "Don't Delete, not Cancel"
    );
    // The question's button, not the toolbar's Delete beneath it.
    let delete = t!("confirm-cache-off-button");
    let buttons = harness.query_all_by_role_and_label(Role::Button, &delete);
    buttons.last().unwrap().click();
    settle(&mut harness);
    assert!(
        harness
            .query_by_label(&t!("confirm-cache-off", count = 1))
            .is_none()
    );
    assert_eq!(cached_count(dir.path()), 0);
}

#[test]
fn with_caching_off_pages_are_sent_again_and_not_stored() {
    let (fetch, calls) = counting_fetch();
    let dir = test_dir("cache_skipped");
    let mut harness = cached_harness(fetch, &cache(dir.path()));
    drop_files(&mut harness, &[("a.png", page_photo())]);
    show_transcript(&mut harness);
    press_command(&mut harness, egui::Key::Comma);
    switch_caching_off(&mut harness);
    harness.get_by_label(&t!("confirm-cache-off-keep")).click();
    settle(&mut harness);
    assert_eq!(cached_count(dir.path()), 1, "Don't Delete kept it");
    let entry = std::fs::read_dir(cache(dir.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let stored = std::fs::metadata(&entry).unwrap().modified().unwrap();

    // The same photo again, once it's no longer a page.
    press_command(&mut harness, egui::Key::Backspace);
    harness.get_by_label(&t!("confirm-delete-button")).click();
    settle(&mut harness);
    drop_files(&mut harness, &[("b.png", page_photo())]);
    click_command(&harness, Command::Transcribe);
    step_until(&mut harness, "# Thurs 23 Oct");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "sent again");
    assert_eq!(
        events(&harness),
        [
            "transcribed from_cache=false",
            "transcribed from_cache=false"
        ]
    );
    let modified = std::fs::metadata(&entry).unwrap().modified().unwrap();
    assert_eq!(modified, stored, "not stored");
}

#[test]
fn switching_caching_off_with_nothing_cached_just_saves() {
    let dir = test_dir("cache_off_empty");
    let mut harness = harness_for(
        NotebookApp::with_transcription(Some(transcriber()), Some(Cache::new(cache(dir.path()))))
            .with_settings_file(dir.path().join("settings.json")),
    );
    press_command(&mut harness, egui::Key::Comma);
    harness.get_by_label(&t!("settings-cache-count", count = 0));
    assert!(disabled(&harness, &t!("settings-cache-clear")));
    switch_caching_off(&mut harness);
    assert!(
        harness
            .query_by_role_and_label(Role::Button, &t!("confirm-cache-off-button"))
            .is_none()
    );
    assert!(
        harness.query_by_label(&t!("settings-save")).is_none(),
        "saved"
    );
}

#[test]
fn tab_reaches_the_cache_settings() {
    let dir = test_dir("cache_tab");
    let mut harness = cached_settings(dir.path());
    tab_to(&mut harness, &t!("settings-cache-location"));
    tab_to(&mut harness, &t!("settings-cache-enable"));
    tab_to(&mut harness, &t!("settings-cache-clear"));
    press(&mut harness, egui::Modifiers::NONE, egui::Key::Space);
    settle(&mut harness);
    harness.get_by_label(&t!("confirm-clear-cache", count = 1));
}
