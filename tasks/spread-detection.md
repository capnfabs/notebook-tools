# Decide: one page vs two-page spread detection

Options: local heuristic (e.g. fold/gutter line via `imageproc`) vs asking Claude (page count + corners). User finds the Claude approach elegant but wants graceful degradation and to avoid leaning too hard on the remote service. Log the outcome in `decisions/`.
