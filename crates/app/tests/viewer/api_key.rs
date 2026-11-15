//! Asking for a Claude API key, keeping it in the keychain, and Settings' API key section. See
//! `decisions/2026-11-15-api-key-in-keychain.md`.

use eframe::egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::{CheckKey, Command, NotebookApp, t};
use notebook_core::claude_transcribe::OcrError;
use notebook_core::credentials::{Credentials, KeyStore as _, Source, Stored};
use std::sync::{Arc, RwLock};

use crate::common::*;

const KEY: &str = "sk-ant-api03-pasted-key-a1B2";

fn asking(harness: &Harness<'_, NotebookApp>) -> bool {
    harness.query_by_label(&t!("api-key-field")).is_some()
}

/// Types `key` into the dialog's field.
fn enter_key(harness: &mut Harness<'_, NotebookApp>, key: &str) {
    let label = t!("api-key-field");
    let field = harness.get_by_label(&label);
    field.focus();
    field.type_text(key);
    harness.run_ok();
}

fn save_key(harness: &mut Harness<'_, NotebookApp>) {
    harness.get_by_label(&t!("api-key-save")).click();
    settle(harness);
}

fn open_settings(harness: &mut Harness<'_, NotebookApp>) {
    press_command(harness, Key::Comma);
}

fn rejecting() -> CheckKey {
    Arc::new(|_| {
        Err(OcrError::Api {
            status: 401,
            kind: "authentication_error".into(),
            message: "invalid x-api-key".into(),
        })
    })
}

#[test]
fn transcribing_without_a_key_asks_for_one_then_carries_on() {
    let mut app = keyed(None, Stored::Empty, any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    assert!(asking(harness));
    assert!(
        harness
            .query_by_label(&t!("api-key-console-link"))
            .is_some(),
        "says where to get a key"
    );

    enter_key(harness, KEY);
    harness.key_press(Key::Enter);
    step_until(harness, "# Thurs 23 Oct");

    assert!(!asking(harness));
    assert_eq!(app.store.load(), Stored::Has(KEY.into()));
    assert_eq!(
        *app.sent_with.lock().unwrap(),
        [Credentials::ApiKey(KEY.into())]
    );
}

/// Switching to the Transcribed view transcribes the page if it can, but doesn't ask for a key:
/// the user may only be looking.
#[test]
fn viewing_transcripts_without_a_key_waits_to_be_asked() {
    let mut app = keyed(None, Stored::Empty, any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    click_command(harness, Command::ViewTranscribed);
    settle(harness);
    assert!(!asking(harness));

    harness.get_by_label(&Command::Transcribe.label()).click();
    settle(harness);
    assert!(asking(harness));
}

#[test]
fn transcribe_all_asks_for_a_key_then_transcribes() {
    let mut app = keyed(None, Stored::Empty, any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo()), ("b.png", page_photo())]);
    click_command(harness, Command::TranscribeAll);
    settle(harness);
    enter_key(harness, KEY);
    save_key(harness);
    step_until_transcribed(harness);
    assert_eq!(app.sent_with.lock().unwrap().len(), 2);
}

#[test]
fn a_rejected_key_isnt_saved() {
    let mut app = keyed(None, Stored::Empty, rejecting());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    enter_key(harness, "sk-ant-wrong");
    save_key(harness);

    assert!(asking(harness), "still asking");
    assert!(harness.query_by_label(&t!("api-key-rejected")).is_some());
    assert_eq!(app.store.load(), Stored::Empty);

    press(harness, Modifiers::NONE, Key::Escape);
    assert!(!asking(harness));
    assert!(app.sent_with.lock().unwrap().is_empty(), "nothing sent");
}

/// Offline, or the API's busy: the key may be fine, so it's saved, and the user is told it wasn't
/// checked.
#[test]
fn a_key_that_couldnt_be_checked_is_saved_with_a_warning() {
    let check: CheckKey = Arc::new(|_| Err(OcrError::Network("offline".into())));
    let mut app = keyed(None, Stored::Empty, check);
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    enter_key(harness, KEY);
    save_key(harness);

    assert!(!asking(harness));
    assert_eq!(app.store.load(), Stored::Has(KEY.into()));
    let warning = before_mark(t!("api-key-unverified", error = MARK));
    assert!(harness.query_by_label_contains(&warning).is_some());
    step_until(harness, "# Thurs 23 Oct");
}

#[test]
fn a_key_the_keychain_wont_take_says_why() {
    let mut app = keyed(None, Stored::Empty, any_key());
    app.store.fail_with("locked");
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    enter_key(harness, KEY);
    save_key(harness);

    assert!(asking(harness), "still asking");
    let error = t!("api-key-save-failed", error = "locked");
    assert!(harness.query_by_label(&error).is_some());
    assert!(app.sent_with.lock().unwrap().is_empty(), "nothing sent");
}

#[test]
fn without_a_keychain_it_says_to_use_the_environment() {
    let stored = Stored::Unavailable("no Secret Service".into());
    let mut app = keyed(None, stored, any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);

    assert!(!asking(harness), "no field to type a key in");
    assert!(harness.query_by_label(&t!("api-key-no-keychain")).is_some());
    let why = t!("api-key-no-keychain-why", error = "no Secret Service");
    assert!(harness.query_by_label(&why).is_some());

    harness.get_by_label(&t!("api-key-close")).click();
    settle(harness);
    assert!(
        harness.query_by_label(&t!("api-key-no-keychain")).is_none(),
        "closed"
    );
}

#[test]
fn checking_a_key_shows_progress() {
    let gate = Arc::new(RwLock::new(()));
    let held = gate.clone();
    let check: CheckKey = Arc::new(move |_| {
        let _open = held.read().unwrap();
        Ok(())
    });
    let mut app = keyed(None, Stored::Empty, check);
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    enter_key(harness, KEY);

    let closed = gate.write().unwrap();
    harness.get_by_label(&t!("api-key-save")).click();
    // The spinner keeps repainting, so the UI never goes idle.
    harness.step();
    harness.step();
    assert!(harness.query_by_label(&t!("api-key-checking")).is_some());
    assert!(disabled(harness, &t!("api-key-save")));
    drop(closed);

    step_until(harness, "# Thurs 23 Oct");
    assert_eq!(app.store.load(), Stored::Has(KEY.into()));
}

#[test]
fn the_key_dialog_works_from_the_keyboard() {
    let mut app = keyed(None, Stored::Empty, any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    assert_eq!(focused(harness), Some(t!("api-key-field")), "field focused");

    tab_to(harness, &t!("api-key-console-link"));
    tab_to(harness, &t!("dialog-cancel"));
    press(harness, Modifiers::NONE, Key::Escape);
    assert!(!asking(harness), "Escape closed it");

    press_command(harness, Key::T);
    enter_key(harness, KEY);
    tab_to(harness, &t!("api-key-save"));
    press(harness, Modifiers::NONE, Key::Enter);
    step_until(harness, "# Thurs 23 Oct");
    assert_eq!(app.store.load(), Stored::Has(KEY.into()));
}

#[test]
fn settings_show_the_saved_key_and_remove_it_after_asking() {
    let mut app = keyed(None, Stored::Has(KEY.into()), any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    open_settings(harness);
    let saved = t!("settings-api-key-keychain", key = "sk-ant-…a1B2");
    assert!(harness.query_by_label(&saved).is_some());

    harness.get_by_label(&t!("settings-api-key-remove")).click();
    settle(harness);
    // Settings' Cancel is under the question's: Escape answers the question.
    press(harness, Modifiers::NONE, Key::Escape);
    assert_eq!(app.store.load(), Stored::Has(KEY.into()), "Escape kept it");
    assert!(
        harness.query_by_label(&saved).is_some(),
        "Settings still open"
    );

    harness.get_by_label(&t!("settings-api-key-remove")).click();
    settle(harness);
    harness
        .get_by_label(&t!("confirm-remove-key-button"))
        .click();
    settle(harness);
    assert_eq!(app.store.load(), Stored::Empty);
    assert!(
        harness
            .query_by_label(&t!("settings-api-key-none"))
            .is_some()
    );
    assert!(disabled(harness, &t!("settings-api-key-remove")));

    press(harness, Modifiers::NONE, Key::Escape);
    press_command(harness, Key::T);
    assert!(asking(harness), "transcribing asks for a key again");
}

/// The environment's credentials win, and the keychain isn't read for them: only when the user
/// asks to enter a key.
#[test]
fn settings_show_the_environments_key_and_read_the_keychain_only_when_asked() {
    let env = (
        Credentials::ApiKey("sk-ant-api03-from-env-wxyz".into()),
        Source::EnvApiKey,
    );
    let mut app = keyed(Some(env.clone()), Stored::Has(KEY.into()), any_key());
    let harness = &mut app.harness;
    drop_files(harness, &[("a.png", page_photo())]);
    open_settings(harness);
    let from_env = t!(
        "settings-api-key-env",
        key = "sk-ant-…wxyz",
        variable = "ANTHROPIC_API_KEY"
    );
    assert!(harness.query_by_label(&from_env).is_some());
    assert!(
        disabled(harness, &t!("settings-api-key-remove")),
        "keychain not read yet"
    );

    harness.get_by_label(&t!("settings-api-key-enter")).click();
    settle(harness);
    let note = t!("api-key-env-note", variable = "ANTHROPIC_API_KEY");
    assert!(harness.query_by_label(&note).is_some());
    press(harness, Modifiers::NONE, Key::Escape);
    assert!(!asking(harness), "Escape closed only the key dialog");
    assert!(
        !disabled(harness, &t!("settings-api-key-remove")),
        "keychain read"
    );

    press(harness, Modifiers::NONE, Key::Escape);
    press_command(harness, Key::T);
    step_until(harness, "# Thurs 23 Oct");
    assert_eq!(*app.sent_with.lock().unwrap(), [env.0]);
}

/// A new or opened project starts the app afresh, but keeps where its credentials come from.
#[test]
fn a_new_project_keeps_the_key() {
    let mut app = keyed(None, Stored::Empty, any_key());
    let harness = &mut app.harness;
    press_command(harness, Key::N);
    open_settings(harness);
    assert!(
        harness
            .query_by_label(&t!("settings-api-key-none"))
            .is_some(),
        "Settings still has the API key section"
    );
    press(harness, Modifiers::NONE, Key::Escape);

    drop_files(harness, &[("a.png", page_photo())]);
    press_command(harness, Key::T);
    assert!(asking(harness), "transcribing still asks for a key");
}
