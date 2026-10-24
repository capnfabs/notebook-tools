# Flaky test: deleting a transcribed page

`deleting_a_transcribed_page_asks_first` (`crates/app/tests/viewer.rs:304`) fails about half the time on main: a spinner keeps repainting past `Harness::run`'s max steps.
