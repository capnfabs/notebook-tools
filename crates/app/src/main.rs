//! Starts the app with real credentials, cache, settings file and native dialogs, and the project
//! named on the command line open, if any (`decisions/2026-10-31-open-by-drop-and-argument.md`);
//! or clears the cache and exits, if the command line says to. Tests build [`NotebookApp`] with
//! stand-ins for these instead.

use notebook_app::cli::{self, Launch};
use notebook_app::{NotebookApp, Pick, t};
use notebook_core::cache::Cache;
use notebook_core::project;
use notebook_core::settings::Settings;

fn main() -> eframe::Result {
    env_logger::init();
    let project = match cli::launch(std::env::args_os().skip(1)) {
        Ok(Launch::Open(project)) => project,
        Ok(Launch::ClearCaches) => match cli::clear_caches(Cache::default_dir()) {
            Ok(done) => {
                println!("{done}");
                return Ok(());
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };
    eframe::run_native(
        &t!("app-name"),
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            let (transcriber, cache) = transcription();
            let app = NotebookApp::with_transcription(transcriber, cache)
                .with_save_path(save_dialog())
                .with_file_picker(file_picker());
            let app = match Settings::default_path() {
                Some(path) => app.with_settings_file(path),
                None => app,
            };
            #[cfg(target_os = "macos")]
            let app = app.with_frontend(Box::new(notebook_app::menu::NativeMenu::install(
                &cc.egui_ctx,
            )));
            // Last, so the transcripts are restyled in the settings' style.
            let app = match project {
                Some((path, opened)) => app.with_project(&path, opened),
                None => app,
            };
            Ok(Box::new(app))
        }),
    )
}

/// Asks where to save with the system's save dialog, offering the suggested name's file type.
/// See `decisions/2026-10-23-rfd-save-dialog.md`.
fn save_dialog() -> notebook_app::SavePath {
    std::sync::Arc::new(|suggested| {
        let extension = std::path::Path::new(suggested)
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let kind = match &*extension {
            "pdf" => t!("file-type-pdf"),
            project::EXTENSION => t!("file-type-project"),
            _ => t!("file-type-markdown"),
        };
        rfd::FileDialog::new()
            .add_filter(kind, &[&*extension])
            .set_file_name(suggested)
            .save_file()
    })
}

/// Asks which photos to add, or which project to open, with the system's open dialog.
fn file_picker() -> notebook_app::PickFiles {
    std::sync::Arc::new(|pick| match pick {
        Pick::Photos => rfd::FileDialog::new()
            .add_filter(
                t!("file-type-photos"),
                &["jpg", "jpeg", "png", "heic", "heif"],
            )
            .pick_files()
            .unwrap_or_default(),
        Pick::Project => rfd::FileDialog::new()
            .add_filter(t!("file-type-project"), &[project::EXTENSION])
            .pick_file()
            .into_iter()
            .collect(),
    })
}

/// Transcribes with Claude when credentials are in the environment, caching what comes back.
/// Without credentials, cached transcripts still load: see
/// `decisions/2026-10-31-transcription-cache.md`.
fn transcription() -> (Option<notebook_app::Transcriber>, Option<Cache>) {
    use notebook_app::eval_log;
    use notebook_core::claude_transcribe::{Credentials, fetch_page};

    let cache = Cache::default_dir().map(Cache::new);
    match &cache {
        Some(cache) => log::info!("Caching transcriptions in {}", cache.dir().display()),
        None => log::warn!("No cache directory: transcriptions won't be cached"),
    }
    let Some(credentials) = Credentials::from_env() else {
        log::info!("Transcription off: set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN");
        return (None, cache);
    };
    let fetch = move |bytes: &[u8], params: &_, corners, model| {
        fetch_page(&credentials, bytes, params, corners, model)
    };
    let transcriber = notebook_app::Transcriber::new(std::sync::Arc::new(fetch));
    // Comparing reads other models' cache entries: only worth it if someone's reading the log.
    let transcriber = match &cache {
        Some(cache) if eval_log::enabled() => {
            transcriber.on_transcribe_completed(eval_log::log_comparisons(cache.clone()))
        }
        _ => transcriber,
    };
    (Some(transcriber), cache)
}
