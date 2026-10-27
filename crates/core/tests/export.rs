//! Export output for recorded transcripts.

use std::path::Path;

use notebook_core::export;
use notebook_core::ocr::parse_response;
use notebook_core::transcript::Transcript;

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
