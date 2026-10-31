# Gzipped project files

`.notebook` files are gzipped pretty JSON (`json_file::write_gzipped`, still written atomically), since they hold every transcript and Claude's raw answer and grow with the notebook. Plain-JSON projects are no longer read; they're refused as not a project. `PROJECT_VERSION` stays 1: it versions the JSON, and an older build refuses a gzipped file rather than misreading it. `project::json` gives the JSON inside, for tests (the format snapshot is unchanged) and for opening. Reason: Human Taste (gzip, dropping plain JSON, pretty inside, no version bump).
