//! Deletes every cached transcription, so pages are sent to Claude again. Run by
//! `scripts/clear-cache.sh`.

use notebook_core::cache::Cache;

fn main() -> std::process::ExitCode {
    let Some(dir) = Cache::default_dir() else {
        eprintln!("No cache directory on this system");
        return std::process::ExitCode::FAILURE;
    };
    if !dir.exists() {
        println!("No cache at {}", dir.display());
        return std::process::ExitCode::SUCCESS;
    }
    match Cache::new(&dir).clear() {
        Ok(count) => {
            println!(
                "Removed {count} cached transcriptions from {}",
                dir.display()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Couldn't clear {}: {err}", dir.display());
            std::process::ExitCode::FAILURE
        }
    }
}
