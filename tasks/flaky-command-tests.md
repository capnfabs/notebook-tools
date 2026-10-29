# Flaky: `settle` while a transcription's spinner is up

Under full-suite load, tests that `settle` with a transcription or export in flight sometimes fail with "Harness::run exceeded max_steps": `loading()` doesn't count transcriptions or exports, so `run` meets a spinner. Seen in `command_e_exports_then_reexports` and `command_shift_t_forces_retranscription` (about 1 in 3 full runs at c7341b0), and less often in `enter_picks_the_affirmative_button`, `command_numbers_switch_the_view` and `deleting_a_transcribed_page_asks_first`. Each passes on its own.
