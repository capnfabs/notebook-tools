# Gzip project files

`.notebook` files are pretty JSON (`notebook_core::project`), with each page's whole transcript and Claude's raw answer, so they grow with the notebook. Write them gzipped. Keep opening plain-JSON ones (tell by the gzip magic bytes) so files saved before this still open, and keep the writes atomic (`json_file::write`). The format snapshot test (`crates/core/tests/project.rs`) should still snapshot the JSON inside.
