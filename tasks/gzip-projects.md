# Gzip project files

`.notebook` files are pretty JSON (`notebook_core::project`), with each page's whole transcript and Claude's raw answer, so they grow with the notebook. Write them gzipped. You can drop support for the existing plain-JSON format. Keep the writes atomic (`json_file::write`). The format snapshot test (`crates/core/tests/project.rs`) should still snapshot the JSON inside.
