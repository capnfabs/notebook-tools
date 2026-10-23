use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use notebook_app::NotebookApp;

#[test]
fn greets_and_counts_clicks() {
    let mut harness = Harness::new_ui_state(
        |ui, app: &mut NotebookApp| app.ui(ui),
        NotebookApp::default(),
    );

    harness.get_by_label("Hello, notebook!");
    harness.get_by_label("Clicked 0 times");

    harness.get_by_label("Click me").click();
    harness.run();

    harness.get_by_label("Clicked 1 times");
}
