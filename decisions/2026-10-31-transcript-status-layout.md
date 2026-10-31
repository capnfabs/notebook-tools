# Transcript status layout

Reset and Force retranscription sit at the bottom right of the page's side of the split. The status text goes to their left, left-aligned, with its last line level with them, as long as it takes two lines or fewer when wrapped to that room. Otherwise it goes above them at full width. Each line takes its own colour: where the transcript came from stays the normal text colour, the bounds warnings take the warning colour, and a failed retranscription takes the error colour. The layout leans on egui's own layouts: the buttons go in first, right to left and aligned to the bottom, and the text gets the room they leave. The text is laid out once at that width, to count its lines, and that galley is the one shown beside the buttons.

Reason: Human Taste (the placement, the two-line rule, per-line colours, using egui's layouts).
