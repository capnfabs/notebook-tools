use notebook_app::NotebookApp;
use notebook_core::cache::Cache;

fn main() -> eframe::Result {
    env_logger::init();
    eframe::run_native(
        "notebook-tools",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            let (transcriber, cache) = transcription();
            let app = NotebookApp::with_transcription(transcriber, cache)
                .with_save_path(save_dialog())
                .with_file_picker(file_picker());
            #[cfg(target_os = "macos")]
            let app = app.with_frontend(Box::new(notebook_app::menu::NativeMenu::install(
                &cc.egui_ctx,
            )));
            Ok(Box::new(app))
        }),
    )
}

/// Asks where to save with the system's save dialog, offering the suggested name's file type.
fn save_dialog() -> notebook_app::SavePath {
    std::sync::Arc::new(|suggested| {
        let extension = std::path::Path::new(suggested)
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let kind = if extension == "pdf" {
            "PDF"
        } else {
            "Markdown"
        };
        rfd::FileDialog::new()
            .add_filter(kind, &[&*extension])
            .set_file_name(suggested)
            .save_file()
    })
}

/// Asks which photos to add with the system's open dialog.
fn file_picker() -> notebook_app::PickFiles {
    std::sync::Arc::new(|| {
        rfd::FileDialog::new()
            .add_filter("Photos", &["jpg", "jpeg", "png", "heic", "heif"])
            .pick_files()
            .unwrap_or_default()
    })
}

/// Transcribes with Claude when credentials are in the environment, caching what comes back.
/// Without credentials there's no cache either, for now: see `tasks/cache-without-credentials.md`.
fn transcription() -> (Option<notebook_app::Transcriber>, Option<Cache>) {
    use notebook_app::eval_log;
    use notebook_core::ocr::{Credentials, fetch_page};

    let Some(credentials) = Credentials::from_env() else {
        log::info!("Transcription off: set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN");
        return (None, None);
    };
    let cache = Cache::default_dir().map(Cache::new);
    match &cache {
        Some(cache) => log::info!("Caching transcriptions in {}", cache.dir().display()),
        None => log::warn!("No cache directory: transcriptions won't be cached"),
    }
    let fetch =
        move |bytes: &[u8], params: &_, model| fetch_page(&credentials, bytes, params, model);
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
