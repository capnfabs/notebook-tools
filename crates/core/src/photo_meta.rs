//! What a photo's file says about it without decoding it: its upright size, and the EXIF that
//! tells the same photo again after it's been moved, re-encoded or resized ([`PhotoIds`]). Read
//! when a page is added, and saved with it.
//!
//! EXIF comes through `kamadak-exif` on every platform, from JPEG, PNG and HEIC alike. Apple's
//! PhotoIdentifier is in its MakerNote, which nothing standard parses, so [`apple_photo_identifier`]
//! does. See `decisions/2026-11-17-relink-photos.md`.

use std::io::Cursor;

use exif::{Exif, In, Tag, Value};
use serde::{Deserialize, Serialize};

use crate::decode::{self, DecodeError};

/// Apple's MakerNote tag for the UUID every iPhone photo gets, kept by its edits and copies.
const PHOTO_IDENTIFIER: u16 = 0x002b;

/// What a photo's file says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhotoMeta {
    /// Width and height once turned upright, as decoding gives them: what corners are measured in.
    pub size: (u32, u32),
    pub exif: PhotoIds,
}

/// The EXIF fields that tell one photo from another, each as the file has it, if it does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhotoIds {
    /// Apple's MakerNote `0x002b`, a UUID: on every iPhone HEIC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_unique_id: Option<String>,
    /// As EXIF writes it, `2024:05:01 12:34:56`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_time_original: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_sec_time_original: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_time_original: Option<String>,
}

impl PhotoIds {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether these say which photo this is: an ID, or when it was taken.
    pub fn identify(&self) -> bool {
        self.photo_identifier.is_some()
            || self.image_unique_id.is_some()
            || self.date_time_original.is_some()
    }

    /// Whether `other` looks like the same photo: every field the same, missing from both or
    /// equal, and enough of them there to say which photo it is.
    pub fn same_photo(&self, other: &PhotoIds) -> bool {
        self == other && self.identify()
    }
}

/// The fewest pixels a photo can have on a side to be a page: fewer can't be read, and a pixel's
/// rounding would be too much of a side to tell one shape from another
/// ([`crate::relink::Shape`]). See `decisions/2026-11-18-aspect-names-and-min-size.md`.
pub const MIN_SIDE: u32 = 200;

/// What `bytes`, a JPEG, PNG or HEIC, say about the photo, from their headers. An error if they
/// aren't an image that can be decoded, or are smaller than [`MIN_SIDE`]; no EXIF is fine.
pub fn read(bytes: &[u8]) -> Result<PhotoMeta, DecodeError> {
    let size = decode::size(bytes)?;
    if size.0.min(size.1) < MIN_SIDE {
        return Err(DecodeError::TooSmall { size });
    }
    Ok(PhotoMeta {
        size,
        exif: read_ids(bytes),
    })
}

/// The photo's [`PhotoIds`], or none if its EXIF can't be read.
fn read_ids(bytes: &[u8]) -> PhotoIds {
    match exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        Ok(exif) => ids(&exif),
        Err(err) => {
            log::debug!("no EXIF: {err}");
            PhotoIds::default()
        }
    }
}

fn ids(exif: &Exif) -> PhotoIds {
    let text = |tag| ascii(&exif.get_field(tag, In::PRIMARY)?.value);
    let maker_note = exif.get_field(Tag::MakerNote, In::PRIMARY);
    PhotoIds {
        photo_identifier: maker_note.and_then(|field| match &field.value {
            Value::Undefined(bytes, _) => apple_photo_identifier(bytes),
            _ => None,
        }),
        image_unique_id: text(Tag::ImageUniqueID),
        date_time_original: text(Tag::DateTimeOriginal),
        sub_sec_time_original: text(Tag::SubSecTimeOriginal),
        offset_time_original: text(Tag::OffsetTimeOriginal),
    }
}

/// An ASCII field's first string, without the padding and NULs cameras leave, if it isn't empty.
fn ascii(value: &Value) -> Option<String> {
    let Value::Ascii(strings) = value else {
        return None;
    };
    text(strings.first()?)
}

fn text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!text.is_empty()).then(|| text.to_owned())
}

/// The PhotoIdentifier in Apple's MakerNote `note`, if it has one: `Apple iOS\0`, a version, a
/// byte order (`MM` or `II`), then an IFD whose offsets count from the start of the note.
pub fn apple_photo_identifier(note: &[u8]) -> Option<String> {
    const HEADER: &[u8] = b"Apple iOS\0";
    let rest = note.strip_prefix(HEADER)?;
    let big = match rest.get(2..4)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = note.get(at..at + 2)?.try_into().ok()?;
        Some(if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = note.get(at..at + 4)?.try_into().ok()?;
        Some(if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };
    const ASCII: u16 = 2;
    let ifd = HEADER.len() + 4;
    let entries = usize::from(u16_at(ifd)?);
    (0..entries).find_map(|i| {
        let entry = ifd + 2 + i * 12;
        if u16_at(entry)? != PHOTO_IDENTIFIER || u16_at(entry + 2)? != ASCII {
            return None;
        }
        let count = usize::try_from(u32_at(entry + 4)?).ok()?;
        let at = if count <= 4 {
            entry + 8
        } else {
            usize::try_from(u32_at(entry + 8)?).ok()?
        };
        text(note.get(at..at.checked_add(count)?)?)
    })
}

/// Photos with EXIF, made up for tests, so no real photo (and its metadata) is committed.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use exif::experimental::Writer;
    use exif::{Field, In, Tag, Value};
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageEncoder as _, RgbImage};

    use super::{PHOTO_IDENTIFIER, PhotoIds};

    /// Apple's MakerNote holding `id` as its PhotoIdentifier, big-endian as iPhones write it,
    /// after an entry before it as real ones have.
    pub fn apple_maker_note(id: &str) -> Vec<u8> {
        let mut note = b"Apple iOS\0\0\x01MM".to_vec();
        let value = [id.as_bytes(), b"\0"].concat();
        let entries: u16 = 2;
        // Header, count, entries, next IFD.
        let data_at = note.len() + 2 + usize::from(entries) * 12 + 4;
        note.extend(entries.to_be_bytes());
        // MakerNoteVersion, SLONG 1, inline.
        note.extend([0x00, 0x01, 0x00, 0x09, 0, 0, 0, 1, 0, 0, 0, 14]);
        note.extend(PHOTO_IDENTIFIER.to_be_bytes());
        note.extend(2u16.to_be_bytes());
        note.extend((value.len() as u32).to_be_bytes());
        note.extend((data_at as u32).to_be_bytes());
        note.extend([0; 4]);
        note.extend(value);
        note
    }

    /// The TIFF-structured EXIF block holding `ids`, as a JPEG's APP1 or a HEIC's `Exif` item
    /// carry it.
    pub fn exif_block(ids: &PhotoIds) -> Vec<u8> {
        let ascii = |tag, text: &Option<String>| {
            text.as_ref().map(|text| Field {
                tag,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![text.as_bytes().to_vec()]),
            })
        };
        let mut fields: Vec<Field> = [
            ascii(Tag::ImageUniqueID, &ids.image_unique_id),
            ascii(Tag::DateTimeOriginal, &ids.date_time_original),
            ascii(Tag::SubSecTimeOriginal, &ids.sub_sec_time_original),
            ascii(Tag::OffsetTimeOriginal, &ids.offset_time_original),
        ]
        .into_iter()
        .flatten()
        .collect();
        if let Some(id) = &ids.photo_identifier {
            fields.push(Field {
                tag: Tag::MakerNote,
                ifd_num: In::PRIMARY,
                value: Value::Undefined(apple_maker_note(id), 0),
            });
        }
        // A TIFF needs at least one field in its first IFD.
        let orientation = Field {
            tag: Tag::Orientation,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![1]),
        };
        let mut writer = Writer::new();
        writer.push_field(&orientation);
        for field in &fields {
            writer.push_field(field);
        }
        let mut block = std::io::Cursor::new(Vec::new());
        writer.write(&mut block, false).unwrap();
        block.into_inner()
    }

    /// `img` as a JPEG whose EXIF holds `ids`.
    pub fn jpeg(img: &RgbImage, ids: &PhotoIds) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 95);
        encoder.set_exif_metadata(exif_block(ids)).unwrap();
        let (w, h) = img.dimensions();
        encoder
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    /// Just enough of a HEIC for its EXIF to be read: a file type, and a `meta` box whose one
    /// item is the `Exif` block holding `ids`, kept in the box itself. It has no image.
    pub fn heic_with_exif_only(ids: &PhotoIds) -> Vec<u8> {
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            [&(body.len() as u32 + 8).to_be_bytes()[..], kind, body].concat()
        }
        /// Version and flags, all zero but the version.
        fn full(version: u8) -> [u8; 4] {
            [version, 0, 0, 0]
        }
        // The block starts with how far into it the TIFF header is.
        let item = [&0u32.to_be_bytes()[..], &exif_block(ids)].concat();
        let infe = boxed(
            b"infe",
            &[&full(2)[..], &1u16.to_be_bytes(), &[0, 0], b"Exif", b"\0"].concat(),
        );
        let iinf = boxed(
            b"iinf",
            &[&full(0)[..], &1u16.to_be_bytes(), &infe].concat(),
        );
        // Version 1, so the item can be in `idat`: 4-byte offsets and lengths, no base offset.
        let iloc = boxed(
            b"iloc",
            &[
                &full(1)[..],
                &[0x44, 0x00],
                &1u16.to_be_bytes(), // items
                &1u16.to_be_bytes(), // item ID
                &[0x00, 0x01],       // construction method 1: in `idat`
                &0u16.to_be_bytes(), // data reference
                &1u16.to_be_bytes(), // extents
                &0u32.to_be_bytes(),
                &(item.len() as u32).to_be_bytes(),
            ]
            .concat(),
        );
        let idat = boxed(b"idat", &item);
        let meta = boxed(b"meta", &[&full(0)[..], &iinf, &iloc, &idat].concat());
        let ftyp = boxed(b"ftyp", b"heic\0\0\0\0mif1heic");
        [ftyp, meta].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::test_images::png;
    use image::{Rgb, RgbImage};

    fn all_ids() -> PhotoIds {
        PhotoIds {
            photo_identifier: Some("6F1C2A8E-1B4D-4C1A-9E77-0D3B5A2F9C10".into()),
            image_unique_id: Some("0123456789abcdef".into()),
            date_time_original: Some("2026:05:01 12:34:56".into()),
            sub_sec_time_original: Some("123".into()),
            offset_time_original: Some("+10:00".into()),
        }
    }

    fn jpeg_of(w: u32, h: u32, ids: &PhotoIds) -> Vec<u8> {
        jpeg(&RgbImage::from_pixel(w, h, Rgb([200, 100, 50])), ids)
    }

    #[test]
    fn reads_every_id_from_a_jpeg() {
        let meta = read(&jpeg_of(400, 200, &all_ids())).unwrap();
        assert_eq!(meta.size, (400, 200));
        assert_eq!(meta.exif, all_ids());
    }

    #[test]
    fn reads_ids_from_a_heic() {
        let ids = read_ids(&heic_with_exif_only(&all_ids()));
        assert_eq!(ids, all_ids());
    }

    #[test]
    fn reads_some_ids() {
        let ids = PhotoIds {
            date_time_original: Some("2019:01:02 03:04:05".into()),
            ..PhotoIds::default()
        };
        assert_eq!(read(&jpeg_of(200, 200, &ids)).unwrap().exif, ids);
    }

    #[test]
    fn photos_without_exif_have_no_ids() {
        let meta = read(&png(300, 200)).unwrap();
        assert_eq!(meta.size, (300, 200));
        assert!(meta.exif.is_empty());
    }

    #[test]
    fn photos_smaller_than_the_minimum_are_errors() {
        for (w, h) in [(199, 300), (300, 199), (1, 1)] {
            assert!(
                matches!(read(&png(w, h)), Err(DecodeError::TooSmall { size }) if size == (w, h)),
                "{w}×{h}"
            );
        }
        assert!(read(&png(MIN_SIDE, MIN_SIDE)).is_ok());
    }

    #[test]
    fn files_that_arent_photos_are_errors() {
        assert!(read(b"just some notes").is_err());
    }

    #[test]
    fn finds_the_photo_identifier_in_apples_maker_note() {
        let note = apple_maker_note("ABC-123");
        assert_eq!(apple_photo_identifier(&note).as_deref(), Some("ABC-123"));
    }

    #[test]
    fn reads_little_endian_maker_notes() {
        let mut note = b"Apple iOS\0\x01\0II".to_vec();
        note.extend(1u16.to_le_bytes());
        note.extend(PHOTO_IDENTIFIER.to_le_bytes());
        note.extend(2u16.to_le_bytes());
        note.extend(4u32.to_le_bytes());
        note.extend(b"AB1\0");
        note.extend([0; 4]);
        assert_eq!(apple_photo_identifier(&note).as_deref(), Some("AB1"));
    }

    #[test]
    fn ignores_broken_maker_notes() {
        let note = apple_maker_note("ABC-123");
        assert_eq!(
            apple_photo_identifier(b"Canon\0\0\0\0\0\0\x01MM\0\x01"),
            None
        );
        assert_eq!(apple_photo_identifier(&note[..note.len() - 30]), None);
        assert_eq!(apple_photo_identifier(&note[..16]), None);
        let mut wrong_order = note.clone();
        wrong_order[12..14].copy_from_slice(b"XX");
        assert_eq!(apple_photo_identifier(&wrong_order), None);
    }

    #[test]
    fn same_photo_when_every_id_agrees() {
        let ids = all_ids();
        assert!(ids.same_photo(&all_ids()));
        let differs = |change: fn(&mut PhotoIds)| {
            let mut other = all_ids();
            change(&mut other);
            !ids.same_photo(&other) && !other.same_photo(&ids)
        };
        assert!(differs(|i| i.photo_identifier = Some("other".into())));
        assert!(differs(|i| i.photo_identifier = None));
        assert!(differs(|i| i.image_unique_id = None));
        assert!(differs(|i| i.sub_sec_time_original = Some("124".into())));
        assert!(differs(|i| i.sub_sec_time_original = None));
        assert!(differs(|i| i.offset_time_original = None));
        assert!(differs(
            |i| i.date_time_original = Some("2026:05:01 12:34:57".into())
        ));
    }

    #[test]
    fn whole_seconds_alone_are_enough() {
        let ids = PhotoIds {
            date_time_original: Some("2019:01:02 03:04:05".into()),
            ..PhotoIds::default()
        };
        assert!(ids.same_photo(&ids.clone()));
    }

    #[test]
    fn photos_without_ids_are_never_the_same() {
        assert!(!PhotoIds::default().same_photo(&PhotoIds::default()));
        let offset_only = PhotoIds {
            offset_time_original: Some("+10:00".into()),
            ..PhotoIds::default()
        };
        assert!(!offset_only.same_photo(&offset_only.clone()));
    }
}
