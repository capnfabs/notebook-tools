# egui on desktop, native shell for mobile later

Desktop UI is egui/eframe: the correction UI only needs to be functional, egui makes image-plus-overlay interaction easy, has the best WASM story, and has `egui_kittest` for UI tests.

All non-UI logic (decode, cleanup, Claude client, export) lives in a UI-agnostic `core` crate with an FFI-friendly API (plain data types, no UI types). If mobile happens it's a thin native shell (e.g. SwiftUI via UniFFI, possibly using VisionKit for capture) rather than egui on mobile.
