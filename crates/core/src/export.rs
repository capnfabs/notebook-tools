//! Transcribed pages as files to save.
//!
//! PDF: each page is the cleaned image with an invisible text layer, one run of text per line, laid
//! over the line's box (`decisions/2026-10-23-line-level-pdf-text.md`). The markdown rides along as
//! an embedded file. See `decisions/2026-10-28-pdf-text-layer.md`.

use krilla::Document;
use krilla::embed::{AssociationKind, EmbeddedFile, MimeType};
use krilla::geom::{Point, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use pulldown_cmark::{Event, Options, Parser};

use crate::transcript::Transcript;

/// Resolution of the page images in a PDF, which sets the pages' physical size.
pub const PDF_DPI: f32 = 300.0;
/// The text layer's font: egui's, as it's already in the binary. It's invisible, so its looks
/// don't matter.
const FONT: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;

/// `pages` as one markdown document, in order. Each page starts with an HTML comment naming its
/// file, which doesn't show when rendered.
pub fn markdown(pages: &[(&str, &Transcript)]) -> String {
    let pages: Vec<String> = pages
        .iter()
        .map(|(name, transcript)| format!("<!-- {name} -->\n{}\n", transcript.markdown()))
        .collect();
    pages.join("\n")
}

/// A transcribed page to put in a PDF: its name, its cleaned image and its transcript.
pub struct PdfPage<'a> {
    pub name: &'a str,
    /// [`crate::Loaded::cleaned`], embedded as it is.
    pub jpeg: &'a [u8],
    /// The JPEG's size in pixels.
    pub size: (u32, u32),
    pub transcript: &'a Transcript,
}

/// `pages` as a searchable PDF, in order: each page's image under an invisible text layer. Their
/// [`markdown`] is embedded as `markdown_name`.
pub fn pdf(pages: &[PdfPage], markdown_name: &str) -> Result<Vec<u8>, String> {
    let font = Font::new(FONT.into(), 0).expect("the bundled font parses");
    let face = ttf_parser::Face::parse(FONT, 0).expect("the bundled font parses");
    let mut document = Document::new();
    for page in pages {
        let (w, h) = page.size;
        let size = (w as f32 / PDF_DPI * 72.0, h as f32 / PDF_DPI * 72.0);
        let settings = PageSettings::from_wh(size.0, size.1)
            .ok_or_else(|| format!("{} is an empty image", page.name))?;
        let mut pdf_page = document.start_page_with(settings);
        let mut surface = pdf_page.surface();
        let image = Image::from_jpeg(page.jpeg.to_vec().into(), true)
            .map_err(|err| format!("Couldn't embed {}: {err}", page.name))?;
        surface.draw_image(image, Size::from_wh(size.0, size.1).expect("checked above"));
        surface.set_fill(Some(Fill {
            opacity: NormalizedF32::ZERO,
            ..Fill::default()
        }));
        for line in text_layer(page.transcript, size) {
            draw_line(&mut surface, &font, &face, &line);
        }
        surface.finish();
        pdf_page.finish();
    }

    let transcripts: Vec<_> = pages.iter().map(|p| (p.name, p.transcript)).collect();
    document.embed_file(EmbeddedFile {
        path: markdown_name.to_owned(),
        mime_type: MimeType::new("text/markdown"),
        description: Some("The transcription as markdown".to_owned()),
        association_kind: AssociationKind::Source,
        data: markdown(&transcripts).into_bytes().into(),
        modification_date: None,
        compress: None,
        location: None,
    });
    document
        .finish()
        .map_err(|err| format!("Couldn't build the PDF: {err:?}"))
}

/// A line of a PDF page's text layer.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLine {
    /// The line's [`plain_text`].
    pub text: String,
    /// `[x0, y0, x1, y1]` in points from the page's top left.
    pub rect: [f32; 4],
}

/// Where `transcript`'s lines go on a page `page_size` points across and down: each line's box,
/// scaled from the image Claude saw to the page. Lines without a box or text are left out. Boxes
/// are upright, even for a transcript reframed onto a turned crop; see
/// `decisions/2026-10-30-pdf-text-upright-boxes.md`.
pub fn text_layer(transcript: &Transcript, page_size: (f32, f32)) -> Vec<PlacedLine> {
    let (w, h) = transcript.crop.size;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let (sx, sy) = (page_size.0 / w as f32, page_size.1 / h as f32);
    transcript
        .lines
        .iter()
        .filter_map(|line| {
            let [x0, y0, x1, y1] = line.bbox?;
            let text = plain_text(&line.text_md);
            (!text.is_empty() && x1 > x0 && y1 > y0).then_some(PlacedLine {
                text,
                rect: [x0 * sx, y0 * sy, x1 * sx, y1 * sy],
            })
        })
        .collect()
}

/// Draws `line` so its ascenders and descenders span its box's height and it's stretched across
/// its width, with the surface's fill.
fn draw_line(surface: &mut Surface, font: &Font, face: &ttf_parser::Face, line: &PlacedLine) {
    let [x0, y0, x1, y1] = line.rect;
    let units_per_em = f32::from(face.units_per_em());
    let (ascent, descent) = (f32::from(face.ascender()), f32::from(face.descender()));
    let font_size = (y1 - y0) * units_per_em / (ascent - descent);
    let advance: u32 = line
        .text
        .chars()
        .map(|c| {
            let glyph = face.glyph_index(c).unwrap_or_default();
            u32::from(face.glyph_hor_advance(glyph).unwrap_or_default())
        })
        .sum();
    let width = advance as f32 * font_size / units_per_em;
    if width <= 0.0 {
        return;
    }
    let baseline = y0 + ascent * font_size / units_per_em;
    surface.push_transform(&Transform::from_row(
        (x1 - x0) / width,
        0.0,
        0.0,
        1.0,
        x0,
        baseline,
    ));
    surface.draw_text(
        Point::from_xy(0.0, 0.0),
        font.clone(),
        font_size,
        &line.text,
        false,
        TextDirection::Auto,
    );
    surface.pop();
}

/// `text_md`, one line of markdown, as the plain text it renders to: without list, heading, quote
/// or checkbox markers, emphasis or code marks, or link targets.
pub fn plain_text(text_md: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut text = String::new();
    for event in Parser::new_ext(text_md, options) {
        if let Event::Text(t) | Event::Code(t) = event {
            text.push_str(&t);
        }
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_drops_block_markers() {
        assert_eq!(plain_text("# Thurs 23 Oct"), "Thurs 23 Oct");
        assert_eq!(plain_text("### Notes"), "Notes");
        assert_eq!(plain_text("#hashtag"), "#hashtag");
        assert_eq!(plain_text("- ring the plumber"), "ring the plumber");
        assert_eq!(plain_text("  * nested"), "nested");
        assert_eq!(plain_text("+ plus"), "plus");
        assert_eq!(plain_text("- [ ] todo"), "todo");
        assert_eq!(plain_text("- [x] done"), "done");
        assert_eq!(plain_text("12. twelfth"), "twelfth");
        assert_eq!(plain_text("3) third"), "third");
        assert_eq!(plain_text("> - quoted item"), "quoted item");
        assert_eq!(plain_text("2026 was a year"), "2026 was a year");
    }

    #[test]
    fn plain_text_drops_inline_marks() {
        assert_eq!(plain_text("**bold** and __strong__"), "bold and strong");
        assert_eq!(plain_text("*em* and _em_"), "em and em");
        assert_eq!(plain_text("~~struck~~ `code`"), "struck code");
        assert_eq!(plain_text("- **Buy** milk"), "Buy milk");
        assert_eq!(
            plain_text("see [the docs](https://example.com)"),
            "see the docs"
        );
        assert_eq!(plain_text(r"\*not em\*"), "*not em*");
    }

    #[test]
    fn plain_text_keeps_what_isnt_markup() {
        assert_eq!(plain_text("snake_case"), "snake_case");
        assert_eq!(plain_text("2*3 = 6"), "2*3 = 6");
        assert_eq!(plain_text("a * b"), "a * b");
        assert_eq!(plain_text("[drawing: arrow]"), "[drawing: arrow]");
    }

    #[test]
    fn plain_text_of_nothing() {
        assert_eq!(plain_text(""), "");
        assert_eq!(plain_text("  "), "");
        assert_eq!(plain_text("#"), "");
        assert_eq!(plain_text("- "), "");
    }
}
