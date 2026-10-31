# egui on desktop, native shell for mobile later

Desktop UI is egui/eframe: the correction UI only needs to be functional, egui makes image-plus-overlay interaction easy, and it has `egui_kittest` for UI tests. (It also had the best WASM story, which stopped mattering with `2026-10-24-drop-wasm.md`.)

All non-UI logic (decode, cleanup, Claude client, export) lives in a UI-agnostic `core` crate with an FFI-friendly API (plain data types, no UI types). If mobile happens it's a thin native shell (e.g. SwiftUI via UniFFI, possibly using VisionKit for capture) rather than egui on mobile.

Reason: unquestioned agent recommendation.
