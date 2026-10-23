use eframe::egui;

#[derive(Default)]
pub struct NotebookApp {
    clicks: u32,
}

impl NotebookApp {
    /// The app's UI, separate from [`eframe::App`] so tests can drive it directly.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(notebook_core::greeting("notebook"));
        if ui.button("Click me").clicked() {
            self.clicks += 1;
        }
        ui.label(format!("Clicked {} times", self.clicks));
    }
}

impl eframe::App for NotebookApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| NotebookApp::ui(self, ui));
    }
}
