use notebook_app::NotebookApp;

fn main() -> eframe::Result {
    env_logger::init();
    eframe::run_native(
        "notebook-tools",
        eframe::NativeOptions::default(),
        Box::new(|_cc| {
            let app = NotebookApp::with_transcriber(transcriber()).with_save_path(save_dialog());
            Ok(Box::new(app))
        }),
    )
}

/// Asks where to save with the system's save dialog.
fn save_dialog() -> notebook_app::SavePath {
    std::sync::Arc::new(|suggested| {
        rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(suggested)
            .save_file()
    })
}

/// Transcribes with Claude when credentials are in the environment, caching what comes back.
fn transcriber() -> Option<notebook_app::Transcriber> {
    use notebook_core::cache::Cache;
    use notebook_core::ocr::{Credentials, transcribe_page};

    let Some(credentials) = Credentials::from_env() else {
        log::info!("Transcription off: set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN");
        return None;
    };
    let cache = Cache::default_dir().map(Cache::new);
    match &cache {
        Some(cache) => log::info!("Caching transcriptions in {}", cache.dir().display()),
        None => log::warn!("No cache directory: transcriptions won't be cached"),
    }
    Some(std::sync::Arc::new(move |bytes, params| {
        transcribe_page(&credentials, cache.as_ref(), bytes, params).map_err(|e| e.to_string())
    }))
}
