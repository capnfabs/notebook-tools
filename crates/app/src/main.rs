use notebook_app::NotebookApp;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    eframe::run_native(
        "notebook-tools",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(NotebookApp::default()))),
    )
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
