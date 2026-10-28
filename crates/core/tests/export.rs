//! Export output for recorded transcripts.

use std::path::Path;

use notebook_core::ocr::parse_response;
use notebook_core::transcript::Transcript;
use notebook_core::{CleanParams, clean, decode, export};

fn recorded(name: &str) -> Transcript {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/ocr/{name}.json"));
    parse_response(200, &std::fs::read(path).unwrap(), (1200, 900)).unwrap()
}

#[test]
fn markdown_names_each_page() {
    let first = recorded("success");
    let mut corrected = recorded("success");
    assert!(corrected.resolve(0, "plumbers"));
    insta::assert_snapshot!(export::markdown(&[
        ("IMG_0001.jpeg", &first),
        ("IMG_0002.jpeg", &corrected),
    ]));
}

#[test]
fn markdown_of_one_page() {
    let page = recorded("success");
    let md = export::markdown(&[("a.png", &page)]);
    assert_eq!(md, format!("<!-- a.png -->\n{}\n", page.markdown()));
}

#[test]
fn text_layer_scales_boxes_to_the_page() {
    // Half the image's size, so every coordinate halves.
    let layer = export::text_layer(&recorded("success"), (600.0, 450.0));
    insta::assert_debug_snapshot!(layer);
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn pdf_has_a_searchable_page_each_and_the_markdown() {
    let photo = fixture("page-grey-table.jpg");
    let (first, mut second) = (recorded("success"), recorded("success"));
    assert!(second.resolve(0, "plumbers"));
    let pages = [
        export::PdfPage {
            name: "a.jpg",
            bytes: &photo,
            transcript: &first,
        },
        export::PdfPage {
            name: "b.jpg",
            bytes: &photo,
            transcript: &second,
        },
    ];
    let bytes = export::pdf(&pages, &CleanParams::default(), "notebook.md").unwrap();
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();

    let ids: Vec<_> = pdf.get_pages().into_values().collect();
    assert_eq!(ids.len(), 2);
    let cleaned = clean(&decode(&photo).unwrap(), &CleanParams::default()).image;
    let expected = |px: u32| px as f32 / export::PDF_DPI * 72.0;
    let media_box: Vec<f32> = pdf
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_float().unwrap())
        .collect();
    assert_eq!(
        media_box,
        [
            0.0,
            0.0,
            expected(cleaned.width()),
            expected(cleaned.height())
        ]
    );

    // Text is only drawn through graphics states that make it invisible.
    let page = pdf.get_dictionary(ids[1]).unwrap();
    let resources = page
        .get_deref(b"Resources", &pdf)
        .unwrap()
        .as_dict()
        .unwrap();
    let states = resources.get_deref(b"ExtGState", &pdf).unwrap();
    for (_, state) in states.as_dict().unwrap() {
        let state = pdf.dereference(state).unwrap().1.as_dict().unwrap();
        assert_eq!(state.get(b"ca").unwrap().as_float().unwrap(), 0.0);
    }

    let text = pdf.extract_text(&[2]).unwrap();
    let lines = [
        "Thurs 23 Oct",
        "ring the plumbers re: tap shower",
        "buy eggs",
        "Idea: notebook OCR w/ Claude [drawing: arrow to a box]",
    ];
    let mut rest = text.as_str();
    for line in lines {
        let at = rest
            .find(line)
            .unwrap_or_else(|| panic!("{line:?} not in {text:?}"));
        rest = &rest[at + line.len()..];
    }

    let embedded = embedded_files(&pdf);
    assert_eq!(
        embedded,
        [(
            "notebook.md".to_owned(),
            export::markdown(&[("a.jpg", &first), ("b.jpg", &second)])
        )]
    );
}

/// The PDF's embedded files, by name.
fn embedded_files(pdf: &lopdf::Document) -> Vec<(String, String)> {
    let names = pdf
        .catalog()
        .unwrap()
        .get_deref(b"Names", pdf)
        .unwrap()
        .as_dict()
        .unwrap()
        .get_deref(b"EmbeddedFiles", pdf)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Names")
        .unwrap()
        .as_array()
        .unwrap();
    names
        .chunks(2)
        .map(|pair| {
            let name = String::from_utf8(pair[0].as_str().unwrap().to_vec()).unwrap();
            let spec = pdf.dereference(&pair[1]).unwrap().1.as_dict().unwrap();
            let file = spec
                .get_deref(b"EF", pdf)
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"F")
                .unwrap()
                .as_reference()
                .unwrap();
            let stream = pdf.get_object(file).unwrap().as_stream().unwrap();
            let data = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            (name, String::from_utf8(data).unwrap())
        })
        .collect()
}
