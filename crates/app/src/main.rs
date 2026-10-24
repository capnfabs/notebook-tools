use notebook_app::NotebookApp;

#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
fn save_dialog() -> notebook_app::SavePath {
    std::sync::Arc::new(|suggested| {
        rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(suggested)
            .save_file()
    })
}

/// Transcribes with Claude when credentials are in the environment, caching what comes back.
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    wasm_bindgen_futures::spawn_local(async {
        let canvas = eframe::web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("the_canvas_id"))
            .expect("missing #the_canvas_id")
            .dyn_into::<eframe::web_sys::HtmlCanvasElement>()
            .expect("#the_canvas_id is not a canvas");

        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(NotebookApp::default()))),
            )
            .await
            .expect("failed to start eframe");
    });
}
