//! The macOS app bundle's `Info.plist` agrees with the code: its id is the keychain's service, it
//! runs the binary cargo builds, it's named as the app names itself, and it opens the photos the
//! open dialog offers and the app's projects. See `decisions/2026-11-15-macos-bundle.md`.
#![cfg(target_os = "macos")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use notebook_app::t;
use notebook_core::{credentials, decode, project};
use serde_json::Value;

/// Where the bundle's resources are.
fn macos_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("macos")
}

/// The plist, read with Apple's own tool.
fn plist() -> Value {
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(macos_dir().join("Info.plist"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn strings(value: &Value) -> BTreeSet<&str> {
    let array = value.as_array().unwrap();
    array.iter().map(|v| v.as_str().unwrap()).collect()
}

/// The document type whose name is `name`.
fn document_type<'a>(plist: &'a Value, name: &str) -> &'a Value {
    let types = plist["CFBundleDocumentTypes"].as_array().unwrap();
    let found = types.iter().find(|t| t["CFBundleTypeName"] == name);
    found.unwrap_or_else(|| panic!("no document type {name}"))
}

#[test]
fn it_is_valid() {
    let output = Command::new("plutil")
        .arg("-lint")
        .arg(macos_dir().join("Info.plist"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn it_names_the_app_and_runs_its_binary() {
    let plist = plist();
    assert_eq!(plist["CFBundleIdentifier"], credentials::SERVICE);
    let binary = Path::new(env!("CARGO_BIN_EXE_notebook-tools"));
    assert_eq!(
        plist["CFBundleExecutable"],
        binary.file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(plist["CFBundleName"], t!("app-name"));
    assert_eq!(plist["CFBundleDisplayName"], t!("app-name"));
    assert_eq!(plist["CFBundlePackageType"], "APPL");
}

/// Photos are offered under Open With, without taking over from the user's default app.
#[test]
fn it_opens_the_photos_the_open_dialog_offers() {
    let plist = plist();
    let photos = document_type(&plist, "Photo");
    assert_eq!(photos["LSHandlerRank"], "Alternate");
    // Each extension's system type: add one here when the dialog gains an extension.
    let type_of = |extension: &str| match extension {
        "jpg" | "jpeg" => "public.jpeg",
        "png" => "public.png",
        "heic" => "public.heic",
        "heif" => "public.heif",
        other => panic!("which type is .{other}?"),
    };
    let offered: BTreeSet<&str> = decode::EXTENSIONS.into_iter().map(type_of).collect();
    assert_eq!(strings(&photos["LSItemContentTypes"]), offered);
}

#[test]
fn it_owns_projects() {
    let plist = plist();
    let projects = document_type(&plist, "Notebook Tools Project");
    assert_eq!(projects["LSHandlerRank"], "Owner");
    let declared = plist["UTExportedTypeDeclarations"].as_array().unwrap();
    let [declared] = declared.as_slice() else {
        panic!("one exported type: {declared:?}");
    };
    let id = declared["UTTypeIdentifier"].as_str().unwrap();
    assert_eq!(
        strings(&projects["LSItemContentTypes"]),
        BTreeSet::from([id])
    );
    let extensions = &declared["UTTypeTagSpecification"]["public.filename-extension"];
    assert_eq!(strings(extensions), BTreeSet::from([project::EXTENSION]));
}

/// `scripts/bundle-macos.sh` compiles the Icon Composer icon the plist names, which has every
/// image its layers use.
#[test]
fn the_icon_is_there_with_its_images() {
    let plist = plist();
    let name = plist["CFBundleIconName"].as_str().unwrap();
    assert_eq!(plist["CFBundleIconFile"], name);
    let icon = macos_dir().join(format!("{name}.icon"));
    let json = std::fs::read(icon.join("icon.json")).unwrap();
    let json: Value = serde_json::from_slice(&json).unwrap();
    let mut images = Vec::new();
    for group in json["groups"].as_array().unwrap() {
        for layer in group["layers"].as_array().unwrap() {
            images.extend(layer["image-name"].as_str());
        }
    }
    assert!(!images.is_empty(), "no images in {}", icon.display());
    for image in images {
        let path = icon.join("Assets").join(image);
        assert!(path.is_file(), "{} is missing", path.display());
    }
}
