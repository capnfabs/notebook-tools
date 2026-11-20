//! Giving a page whose photo is missing its photo again: Relink… picks the file, Use Changed File
//! takes the one now at its path, and a dropped file that looks like a missing page's photo by
//! its EXIF asks whether it is. Whether a file matches, and how corners follow it, is
//! `notebook_core::relink`'s. See `decisions/2026-11-17-relink-photos.md`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use notebook_core::page::{Page, PageId};
use notebook_core::photo_meta::{self, PhotoIds, PhotoMeta};
use notebook_core::relink::{self, Aspect, Plan, Ratio, Refusal, Shape, Tier, Turn};

use super::NotebookApp;
use crate::ingest::Pick;
use crate::t;
use crate::ui::dialog::{self, Choice, Stakes};

/// The questions relinking is asking, one at a time.
#[derive(Default)]
pub(super) struct Relinking {
    /// Relink… or Use Changed File found a file that might be the page's photo: relink it?
    confirm: Option<Confirm>,
    /// Why the file picked can't be the page's photo, until the user dismisses it.
    refused: Option<Refused>,
    /// Dropped files that look like the photos of missing pages, each asking which, if any.
    asking: VecDeque<Asking>,
}

struct Confirm {
    id: PageId,
    /// The file, read as a page.
    photo: Page,
    plan: Plan,
}

struct Refused {
    title: String,
    detail: Option<String>,
    /// The photos compared, when that shows why.
    compared: Option<Compared>,
}

struct Asking {
    photo: Page,
    /// The missing pages it looks like, by EXIF, and is the shape of.
    candidates: Vec<PageId>,
    /// Which of them the user picked.
    chosen: usize,
}

impl Relinking {
    pub(super) fn dialog_open(&self) -> bool {
        self.confirm.is_some() || self.refused.is_some() || !self.asking.is_empty()
    }
}

/// A file read as a page, or why it couldn't be.
pub(super) fn read_photo(
    name: String,
    path: Option<PathBuf>,
    bytes: Arc<[u8]>,
) -> Result<Page, String> {
    let meta = photo_meta::read(&bytes).map_err(|e| crate::i18n::decode_error(&e))?;
    let page = Page::new(name, bytes, meta);
    Ok(match path {
        Some(path) => page.at(path),
        None => page,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn size_text((w, h): (u32, u32)) -> String {
    format!("{w}×{h}")
}

/// When the photo was taken, as its EXIF says, to the fraction of a second and with its offset
/// if known.
fn taken(ids: &PhotoIds) -> String {
    match when_taken(ids) {
        Some(when) => t!("relink-taken", when = when),
        None => t!("relink-taken-unknown"),
    }
}

/// The date and time the photo was taken, with its fraction of a second and offset if known:
/// `2026-05-01 12:34:56.250 +10:00`.
fn when_taken(ids: &PhotoIds) -> Option<String> {
    let mut when = exif_date(ids.date_time_original.as_deref()?);
    if let Some(sub) = &ids.sub_sec_time_original {
        when.push('.');
        when.push_str(sub);
    }
    if let Some(offset) = &ids.offset_time_original {
        when.push(' ');
        when.push_str(offset);
    }
    Some(when)
}

/// The fields that say when a photo was taken: date and time, fraction of a second, offset.
fn when_fields(ids: &PhotoIds) -> [Option<&str>; 3] {
    [
        ids.date_time_original.as_deref(),
        ids.sub_sec_time_original.as_deref(),
        ids.offset_time_original.as_deref(),
    ]
}

/// Longest an ID is shown before it's shortened.
const LONGEST_ID: usize = 16;

/// `id`, shortened in the middle if it's long, as UUIDs are: `6F1C2A8E…9C10`.
fn shortened(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= LONGEST_ID {
        return id.to_owned();
    }
    let (start, end) = (&chars[..8], &chars[chars.len() - 4..]);
    format!(
        "{}…{}",
        start.iter().collect::<String>(),
        end.iter().collect::<String>()
    )
}

/// A line about `page`'s photo: its name, size and when it was taken.
fn describe(page: &Page) -> String {
    t!(
        "relink-photo-line",
        name = page.name.as_str(),
        size = size_text(page.meta.size),
        taken = taken(&page.meta.exif)
    )
}

/// A page's photo beside a file, field by field, for the user to judge whether they're the same.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Compared {
    old_name: String,
    new_name: String,
    rows: Vec<Row>,
}

/// One field of two photos, and whether it's the same in both.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    field: String,
    old: String,
    new: String,
    matches: bool,
    /// Each value in full, where it's shortened, to show on hover.
    full: [Option<String>; 2],
}

impl Compared {
    /// `old`, the page's photo, beside `new`, the file. Sizes and shapes always, then each EXIF
    /// field either has.
    pub(super) fn of(old: &Page, new: &Page) -> Self {
        Self {
            old_name: old.name.clone(),
            new_name: new.name.clone(),
            rows: rows(&old.meta, &new.meta),
        }
    }

    /// A grid of the fields, the page's photo then the file, each red where they differ.
    fn show(&self, ui: &mut egui::Ui) {
        ui.add_space(ui.spacing().item_spacing.y);
        egui::Grid::new("relink comparison")
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                ui.label("");
                let old = t!("relink-column-old", name = self.old_name.as_str());
                let new = t!("relink-column-new", name = self.new_name.as_str());
                ui.label(egui::RichText::new(old).strong());
                ui.label(egui::RichText::new(new).strong());
                ui.end_row();
                let mismatch = ui.visuals().error_fg_color;
                for row in &self.rows {
                    ui.label(row.field.as_str());
                    for (value, full) in [&row.old, &row.new].into_iter().zip(&row.full) {
                        let text = egui::RichText::new(value.as_str());
                        let label = ui.label(if row.matches {
                            text
                        } else {
                            text.color(mismatch)
                        });
                        if let Some(full) = full {
                            label.on_hover_text(full.as_str());
                        }
                    }
                    ui.end_row();
                }
            });
    }
}

/// One of a photo's EXIF IDs, if it has it.
type ExifField = fn(&PhotoIds) -> Option<&str>;

fn rows(old: &PhotoMeta, new: &PhotoMeta) -> Vec<Row> {
    let row = |field: String, old: String, new: String, matches: bool| Row {
        field,
        old,
        new,
        matches,
        full: [None, None],
    };
    let mut rows = vec![
        row(
            t!("relink-field-size"),
            size_text(old.size),
            size_text(new.size),
            old.size == new.size,
        ),
        {
            let shape = Shape::of(old.size, new.size);
            let [old, new] = aspect_texts(old.size, new.size, shape);
            row(t!("relink-field-aspect"), old, new, !shape.is_different())
        },
    ];
    let none = || t!("relink-no-value");
    if when_fields(&old.exif) != [None; 3] || when_fields(&new.exif) != [None; 3] {
        let text = |ids: &PhotoIds| when_taken(ids).unwrap_or_else(none);
        let matches = when_fields(&old.exif) == when_fields(&new.exif);
        rows.push(row(
            t!("relink-field-taken"),
            text(&old.exif),
            text(&new.exif),
            matches,
        ));
    }
    let ids: [(String, ExifField); 2] = [
        (t!("relink-field-photo-id"), |ids| {
            ids.photo_identifier.as_deref()
        }),
        (t!("relink-field-unique-id"), |ids| {
            ids.image_unique_id.as_deref()
        }),
    ];
    for (field, value) in ids {
        let (old, new) = (value(&old.exif), value(&new.exif));
        if old.is_none() && new.is_none() {
            continue;
        }
        let text = |id: Option<&str>| id.map_or_else(none, shortened);
        let full = |id: Option<&str>| id.filter(|id| shortened(id) != *id).map(str::to_owned);
        rows.push(Row {
            full: [full(old), full(new)],
            ..row(field, text(old), text(new), old == new)
        });
    }
    rows
}

/// An EXIF date, `2026:05:01 12:34:56`, with dashes in the date as people write it.
fn exif_date(at: &str) -> String {
    let (date, time) = at.split_once(' ').unwrap_or((at, ""));
    format!("{} {time}", date.replace(':', "-"))
        .trim()
        .to_owned()
}

/// Places a ratio that isn't named is written to, unless that hides a difference.
const RATIO_PLACES: usize = 2;

/// Most places a ratio is written to, to tell two shapes apart.
const MOST_RATIO_PLACES: usize = 6;

/// A photo's shape as people say it, long side first, and which way it's turned: `4:3,
/// landscape`, or `1.43:1, portrait` for a ratio that isn't named, to `places`. See
/// `decisions/2026-11-18-aspect-names-and-min-size.md`.
fn aspect_text(aspect: Aspect, places: usize) -> String {
    let ratio = match aspect.ratio {
        Ratio::Named(long, short) => format!("{long}:{short}"),
        Ratio::Other(ratio) => format!("{ratio:.places$}:1"),
    };
    let turned = match aspect.turn {
        Turn::Landscape => t!("relink-landscape"),
        Turn::Portrait => t!("relink-portrait"),
        Turn::Square => t!("relink-square"),
    };
    t!("relink-aspect", ratio = ratio, turned = turned)
}

/// The shapes of the page's photo, `old`, and the file, `new`, which is `shape` beside it, as
/// [`aspect_text`]s that agree exactly when relink does. The same shape, resized, reads as the
/// page's. Another shape that would read the same, being near the same named ratio or rounding
/// alike, is written in places enough to tell them apart.
fn aspect_texts(old_size: (u32, u32), new_size: (u32, u32), shape: Shape) -> [String; 2] {
    let old = Aspect::of(old_size);
    if !shape.is_different() {
        let text = aspect_text(old, RATIO_PLACES);
        return [text.clone(), text];
    }
    let new = Aspect::of(new_size);
    let texts = |old, new, places| [aspect_text(old, places), aspect_text(new, places)];
    let named = texts(old, new, RATIO_PLACES);
    if named[0] != named[1] {
        return named;
    }
    let unnamed = |(w, h): (u32, u32), aspect: Aspect| Aspect {
        ratio: Ratio::Other(f64::from(w.max(h)) / f64::from(w.min(h).max(1))),
        ..aspect
    };
    let (old, new) = (unnamed(old_size, old), unnamed(new_size, new));
    (RATIO_PLACES..=MOST_RATIO_PLACES)
        .map(|places| texts(old, new, places))
        .find(|[old, new]| old != new)
        .unwrap_or_else(|| texts(old, new, MOST_RATIO_PLACES))
}

/// Why the file `new` can't be relinked. A different shape is shown in the table beside it.
fn refusal_text(new: &str, refusal: &Refusal) -> String {
    match refusal {
        Refusal::AlreadyAPage(page) => {
            t!("relink-already-a-page", file = new, page = page.as_str())
        }
        Refusal::DifferentShape { .. } => t!("relink-different-shape"),
    }
}

impl NotebookApp {
    /// Asks which file is the selected page's photo, looking where it was, and relinks it.
    pub(super) fn relink_picked(&mut self, ctx: &egui::Context) {
        let Some(pick_files) = self.pick_files.clone() else {
            return;
        };
        let Some(page) = self.pages.get(self.selected_index()) else {
            return;
        };
        let id = page.id;
        let near = page.path.as_deref().and_then(relink::nearest_existing_dir);
        self.asked_which_files = true;
        if let Some(path) = pick_files(Pick::Relink { near }).into_iter().next() {
            self.relink_from(ctx, id, &path);
        }
    }

    /// Relinks the selected page, whose photo changed, to the file now at its path.
    pub(super) fn use_changed_file(&mut self, ctx: &egui::Context) {
        let Some(page) = self.pages.get(self.selected_index()) else {
            return;
        };
        let id = page.id;
        if let Some(path) = page.path.clone() {
            self.relink_from(ctx, id, &path);
        }
    }

    /// Relinks page `id` to the file at `path`: at once if it's the same file, after asking if
    /// it's another, and not at all if it can't be the page's photo.
    fn relink_from(&mut self, ctx: &egui::Context, id: PageId, path: &Path) {
        let name = file_name(path);
        let read = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| read_photo(name.clone(), Some(path.to_path_buf()), bytes.into()));
        let photo = match read {
            Ok(photo) => photo,
            Err(error) => {
                self.errors
                    .push(t!("open-failed", name = name.as_str(), error = error));
                return;
            }
        };
        match relink::check(&self.pages, id, &photo) {
            Ok(Plan {
                tier: Tier::Hash,
                shape,
            }) => self.relink(ctx, id, &photo, shape),
            Ok(plan) => self.relinking.confirm = Some(Confirm { id, photo, plan }),
            Err(refusal) => {
                let Some(page) = self.pages.find(id) else {
                    return;
                };
                let text = refusal_text(&name, &refusal);
                self.relinking.refused = Some(match refusal {
                    // The table says the rest.
                    Refusal::DifferentShape { .. } => Refused {
                        title: text,
                        detail: None,
                        compared: Some(Compared::of(page, &photo)),
                    },
                    Refusal::AlreadyAPage(_) => Refused {
                        title: t!("relink-refused-title"),
                        detail: Some(text),
                        compared: None,
                    },
                });
            }
        }
    }

    /// Gives page `id` `photo`, of `shape` beside its own, and loads it.
    fn relink(&mut self, ctx: &egui::Context, id: PageId, photo: &Page, shape: Shape) {
        let Some(page) = self.pages.find_mut(id) else {
            return;
        };
        let hint = t!("photo-found", name = page.name.as_str());
        page.relink(photo, shape);
        self.textures.forget(id);
        self.start_loading(id);
        self.selected = Some(id);
        self.show_hint(ctx, hint);
    }

    /// Asks which missing page, if any, `photo`, dropped, is the photo of: one of `candidates`,
    /// which it looks like by EXIF.
    pub(super) fn ask_which_page(&mut self, photo: Page, candidates: Vec<PageId>) {
        self.relinking.asking.push_back(Asking {
            photo,
            candidates,
            chosen: 0,
        });
    }

    /// The relinking dialog showing, if one is.
    pub(super) fn relink_dialogs(&mut self, ctx: &egui::Context) {
        self.refused_dialog(ctx);
        self.confirm_dialog(ctx);
        self.asking_dialog(ctx);
    }

    fn refused_dialog(&mut self, ctx: &egui::Context) {
        let Some(refused) = &self.relinking.refused else {
            return;
        };
        let choice = dialog::show(ctx, "relink refused", 440.0, |ui| {
            ui.label(egui::RichText::new(refused.title.as_str()).strong());
            if let Some(detail) = &refused.detail {
                ui.label(detail.as_str());
            }
            if let Some(compared) = &refused.compared {
                compared.show(ui);
            }
            dialog::only_button(ui, &t!("dialog-ok"))
        });
        if choice.is_some() {
            self.relinking.refused = None;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.relinking.confirm else {
            return;
        };
        let Some(page) = self.pages.find(confirm.id) else {
            self.relinking.confirm = None;
            return;
        };
        let (old, new) = (page.name.as_str(), confirm.photo.name.as_str());
        let (title, button, stakes) = match confirm.plan.tier {
            Tier::Exif => (
                t!("relink-looks-alike", file = new, page = old),
                t!("relink-button"),
                Stakes::Safe,
            ),
            _ => (
                t!("relink-unmatched", file = new, page = old),
                t!("relink-anyway-button"),
                Stakes::Dangerous,
            ),
        };
        let compared = Compared::of(page, &confirm.photo);
        let choice = dialog::show(ctx, "confirm relink", 440.0, |ui| {
            ui.label(egui::RichText::new(title).strong());
            compared.show(ui);
            dialog::buttons(ui, &button, stakes, true)
        });
        if choice.is_none() {
            return;
        }
        let Some(confirm) = self.relinking.confirm.take() else {
            return;
        };
        if choice == Some(Choice::Affirmative) {
            self.relink(ctx, confirm.id, &confirm.photo, confirm.plan.shape);
        }
    }

    /// Asks about the first dropped file that looks like a missing page's photo.
    fn asking_dialog(&mut self, ctx: &egui::Context) {
        let more = self.relinking.asking.len() > 1;
        let Some(asking) = self.relinking.asking.front_mut() else {
            return;
        };
        let candidates: Vec<&Page> = asking
            .candidates
            .iter()
            .filter_map(|&id| self.pages.find(id))
            .filter(|p| p.missing_photo().is_some())
            .collect();
        let photo = &asking.photo;
        let new = photo.name.as_str();
        asking.chosen = asking.chosen.min(candidates.len().saturating_sub(1));
        let cancel = if more {
            t!("dialog-cancel-all")
        } else {
            t!("dialog-cancel")
        };
        let choice = dialog::show(ctx, "relink dropped", 440.0, |ui| {
            match candidates.as_slice() {
                [] => {}
                [page] => {
                    let title = t!("relink-dropped-one", file = new, page = page.name.as_str());
                    ui.label(egui::RichText::new(title).strong());
                }
                pages => {
                    let title = t!("relink-dropped-several", file = new);
                    ui.label(egui::RichText::new(title).strong());
                    for (i, page) in pages.iter().enumerate() {
                        ui.radio_value(&mut asking.chosen, i, describe(page));
                    }
                }
            }
            if let Some(page) = candidates.get(asking.chosen) {
                Compared::of(page, photo).show(ui);
            }
            dialog::buttons_with_other(
                ui,
                &t!("relink-button"),
                &t!("relink-add-as-new-page"),
                &cancel,
                Stakes::Safe,
            )
        });
        let chosen = candidates.get(asking.chosen).map(|p| p.id);
        let Some(choice) = choice else {
            return;
        };
        let Some(asking) = self.relinking.asking.pop_front() else {
            return;
        };
        match (choice, chosen) {
            (Choice::Affirmative, Some(id)) => {
                match relink::check(&self.pages, id, &asking.photo) {
                    Ok(plan) => self.relink(ctx, id, &asking.photo, plan.shape),
                    Err(refusal) => {
                        let text = refusal_text(&asking.photo.name, &refusal);
                        self.errors.push(text);
                    }
                }
            }
            (Choice::Discard, _) | (Choice::Affirmative, None) => self.add_page(asking.photo),
            // Stops asking: no more dialogs for the files left.
            (Choice::Cancel, _) => self.relinking.asking.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(size: (u32, u32), exif: PhotoIds) -> PhotoMeta {
        PhotoMeta { size, exif }
    }

    fn taken(at: &str) -> PhotoIds {
        PhotoIds {
            date_time_original: Some(at.into()),
            sub_sec_time_original: Some("250".into()),
            offset_time_original: Some("+10:00".into()),
            ..PhotoIds::default()
        }
    }

    /// Each row as `(old, new, matches)`.
    fn compared(old: &PhotoMeta, new: &PhotoMeta) -> Vec<(String, String, bool)> {
        rows(old, new)
            .into_iter()
            .map(|r| (r.old, r.new, r.matches))
            .collect()
    }

    #[test]
    fn compares_sizes_shapes_and_the_exif_either_has() {
        let old = meta((4032, 3024), taken("2026:05:01 12:34:56"));
        let mut exif = taken("2026:05:01 12:34:56");
        exif.photo_identifier = Some("ABC".into());
        let new = meta((1008, 756), exif);
        let none = t!("relink-no-value");
        let when = "2026-05-01 12:34:56.250 +10:00";
        assert_eq!(
            compared(&old, &new),
            [
                ("4032×3024".into(), "1008×756".into(), false),
                (four_three(), four_three(), true),
                (when.into(), when.into(), true),
                (none, "ABC".into(), false),
            ]
        );
    }

    #[test]
    fn the_date_row_has_the_fraction_and_offset_too() {
        let old = meta((4, 3), taken("2026:05:01 12:34:56"));
        let other_zone = PhotoIds {
            offset_time_original: Some("+09:00".into()),
            ..taken("2026:05:01 12:34:56")
        };
        let rows = rows(&old, &meta((4, 3), other_zone));
        assert_eq!(rows[2].new, "2026-05-01 12:34:56.250 +09:00");
        assert!(!rows[2].matches, "the offset differs");
    }

    #[test]
    fn long_ids_are_shortened_with_the_whole_on_hover() {
        let uuid = "6F1C2A8E-1B4D-4C1A-9E77-0D3B5A2F9C10";
        assert_eq!(shortened(uuid), "6F1C2A8E…9C10");
        assert_eq!(shortened("0123456789abcdef"), "0123456789abcdef");
        let ids = |id: &str| PhotoIds {
            photo_identifier: Some(id.into()),
            ..PhotoIds::default()
        };
        let rows = rows(&meta((4, 3), ids(uuid)), &meta((4, 3), ids("short")));
        let id = &rows[2];
        assert_eq!(
            (id.old.as_str(), id.new.as_str()),
            ("6F1C2A8E…9C10", "short")
        );
        assert_eq!(id.full, [Some(uuid.to_owned()), None]);
    }

    fn aspect(ratio: &str, turned: String) -> String {
        t!("relink-aspect", ratio = ratio, turned = turned)
    }

    fn four_three() -> String {
        aspect("4:3", t!("relink-landscape"))
    }

    /// The aspect row's `(old, new, matches)` for photos of sizes `old` and `new`.
    fn aspects(old: (u32, u32), new: (u32, u32)) -> (String, String, bool) {
        let rows = rows(
            &meta(old, PhotoIds::default()),
            &meta(new, PhotoIds::default()),
        );
        let row = &rows[1];
        (row.old.clone(), row.new.clone(), row.matches)
    }

    #[test]
    fn another_shape_is_a_mismatch() {
        let rows = rows(
            &meta((4032, 3024), PhotoIds::default()),
            &meta((3024, 4032), PhotoIds::default()),
        );
        assert_eq!(rows.len(), 2, "no EXIF on either, so no EXIF rows");
        assert!(!rows[1].matches);
        assert_eq!(rows[1].old, four_three());
        assert_eq!(rows[1].new, aspect("4:3", t!("relink-portrait")));
    }

    #[test]
    fn shapes_are_named_or_written_to_two_places() {
        let landscape = || t!("relink-landscape");
        assert_eq!(
            aspects((6000, 4000), (6000, 4000)).0,
            aspect("3:2", landscape())
        );
        assert_eq!(
            aspects((2000, 2000), (2000, 2000)).0,
            aspect("1:1", t!("relink-square"))
        );
        assert_eq!(
            aspects((4032, 3000), (4032, 3000)).0,
            aspect("1.34:1", landscape())
        );
    }

    #[test]
    fn a_resized_photo_reads_as_the_pages_shape() {
        assert_eq!(
            aspects((4032, 3024), (1001, 751)),
            (four_three(), four_three(), true)
        );
        // Neither is named, and rounded alone they'd read 1.43:1 and 1.42:1.
        let (old, new, matches) = aspects((4000, 2807), (1000, 702));
        assert!(matches);
        assert_eq!(old, new);
        assert_eq!(old, aspect("1.43:1", t!("relink-landscape")));
    }

    #[test]
    fn other_shapes_never_read_the_same() {
        // Both within a pixel of 4:3, but two pixels apart: relink refuses it.
        let (old, new, matches) = aspects((4001, 3000), (3999, 3000));
        assert!(!matches);
        let landscape = || t!("relink-landscape");
        assert_eq!(old, aspect("1.334:1", landscape()));
        assert_eq!(new, aspect("1.333:1", landscape()));
        // Neither named, and the same to two places.
        let (old, new, matches) = aspects((4000, 2990), (4000, 2995));
        assert!(!matches);
        assert_ne!(old, new);
    }
}
