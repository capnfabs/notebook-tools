# Audit the repo and strip PII

Ensure that there's no references to certain things / people. List all images in the repo, vet that the _back_ side of the photographed pages doesn't contain anything spicy, too.

- [x] Audit all history; items to decide on are in `~/Code/notebook-tools-pii-review/README.md` (outside the repo).
- [x] Guardrails: `decisions/2026-11-01-image-guardrails.md`.
- [x] HEAD: new spread-wood-1/-2 photos, corners, goldens, re-recorded response; bbox-spike test and fixtures removed.
- [ ] Rewrite history in a fresh clone (no `git filter-repo`, so fast-export/filter-branch); approve kept images; push to a new GitHub repo.
