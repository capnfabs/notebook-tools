# Resizable transcript split

A handle between the page and the transcript sets the transcript's width, which is kept across pages. On window resize, the transcript gives up width first, down to its natural width (longest line unwrapped, +20% for corrections); below that, both panes shrink in proportion. Growing refills the transcript to its natural width, then grows the page until it fills its height, then the transcript. Neither pane goes under 200pt; the stacked layout for narrow windows stays 50/50.

Reason: Human Taste (the resize rules); agent recommendation accepted for proportional shrinking and the page's cap.
