# Polish the transcript status line

The status under the page (`transcript_status` in `crates/app/src/app.rs`) is functional but rough:

- The whole block takes the warning colour, including the neutral "From cache: …" line.
- Long warnings get clipped at the panel's bottom edge ([missed-page.png](status-ui/missed-page.png)).
- The "Force retranscription" button floats beside the text rather than sitting with the warning it answers ([lines-cut-off.png](status-ui/lines-cut-off.png)).
