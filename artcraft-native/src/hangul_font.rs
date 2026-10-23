//! A system font with Hangul, placed last in every font family so Korean text renders instead of
//! tofu boxes: the bundled Inter, Archivo and Geist Mono, and egui's own fallbacks, have none.
//!
//! System CJK fonts ship as large collections (Apple SD Gothic Neo is one 55 MB `.ttc` holding
//! 18 faces), and egui keeps the bytes it is given in memory. So only the faces the theme draws
//! are copied out, table by table, into standalone fonts of about 3 MB each.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::ensure;
use log::{info, warn};
use skrifa::{FontRef, MetadataProvider};

/// A face covers Hangul when it maps all of these: the first and last syllables and a common one.
const PROBE: [char; 3] = ['가', '한', '힣'];

/// The Hangul faces at the weights the theme draws. A weight that wasn't found uses regular.
pub struct HangulFonts {
  regular: Vec<u8>,
  medium: Option<Vec<u8>>,
  semibold: Option<Vec<u8>>,
}

impl HangulFonts {
  /// The face for `weight`, or the regular face when that weight wasn't found.
  pub fn face(&self, weight: Weight) -> &[u8] {
    let face = match weight {
      Weight::Regular => None,
      Weight::Medium => self.medium.as_deref(),
      Weight::SemiBold => self.semibold.as_deref(),
    };
    face.unwrap_or(&self.regular)
  }
}

/// The weights the theme's families map to.
#[derive(Clone, Copy)]
pub enum Weight {
  Regular,
  Medium,
  SemiBold,
}

impl Weight {
  pub const ALL: [Weight; 3] = [Weight::Regular, Weight::Medium, Weight::SemiBold];

  /// The name the face is registered under in egui's font definitions.
  pub fn font_name(self) -> &'static str {
    match self {
      Weight::Regular => "Hangul",
      Weight::Medium => "Hangul-Medium",
      Weight::SemiBold => "Hangul-SemiBold",
    }
  }
}

/// The first system font with Hangul, loaded once per process. egui rebuilds its fonts on every
/// scale change; static bytes are borrowed there instead of copied.
pub fn hangul_fonts() -> Option<&'static HangulFonts> {
  static FONTS: OnceLock<Option<HangulFonts>> = OnceLock::new();
  FONTS.get_or_init(load).as_ref()
}

fn load() -> Option<HangulFonts> {
  for candidate in candidates() {
    let Some(regular) = load_face(&candidate.regular) else {
      continue;
    };
    let medium = candidate.medium.as_ref().and_then(load_face);
    let semibold = candidate.semibold.as_ref().and_then(load_face);
    info!("Korean text uses {}", candidate.regular.0.display());
    return Some(HangulFonts { regular, medium, semibold });
  }
  warn!("No system font with Hangul was found; Korean text will show as boxes");
  None
}

/// A system font to try: the file and face index for each weight.
struct Candidate {
  regular: FaceRef,
  medium: Option<FaceRef>,
  semibold: Option<FaceRef>,
}

type FaceRef = (PathBuf, u32);

/// Where each platform keeps a font with Hangul, best first.
#[cfg(target_os = "macos")]
fn candidates() -> Vec<Candidate> {
  // Faces 0, 2 and 4 are Regular, Medium and SemiBold; the odd ones are hidden variants.
  const NEO: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";
  let neo = Candidate { regular: (NEO.into(), 0), medium: Some((NEO.into(), 2)), semibold: Some((NEO.into(), 4)) };
  let gothic = Candidate { regular: ("/System/Library/Fonts/Supplemental/AppleGothic.ttf".into(), 0), medium: None, semibold: None };
  vec![neo, gothic]
}

#[cfg(target_os = "windows")]
fn candidates() -> Vec<Candidate> {
  let fonts = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
  // Malgun Gothic ships regular and bold only.
  vec![Candidate { regular: (fonts.join("malgun.ttf"), 0), medium: None, semibold: Some((fonts.join("malgunbd.ttf"), 0)) }]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn candidates() -> Vec<Candidate> {
  // Noto Sans CJK where Debian/Ubuntu, Arch and Fedora put it (face 1 is the KR face), then
  // Nanum Gothic. Noto's faces share one large glyph table, so only regular is copied.
  let noto = ["/usr/share/fonts/opentype/noto", "/usr/share/fonts/noto-cjk", "/usr/share/fonts/google-noto-cjk"].map(|dir| Candidate { regular: (Path::new(dir).join("NotoSansCJK-Regular.ttc"), 1), medium: None, semibold: None });
  let nanum = Candidate { regular: ("/usr/share/fonts/truetype/nanum/NanumGothic.ttf".into(), 0), medium: None, semibold: Some(("/usr/share/fonts/truetype/nanum/NanumGothicBold.ttf".into(), 0)) };
  noto.into_iter().chain([nanum]).collect()
}

#[cfg(not(any(unix, target_os = "windows")))]
fn candidates() -> Vec<Candidate> {
  Vec::new()
}

/// Copies a face out and checks it has Hangul; egui panics on fonts it can't parse.
fn load_face((path, index): &FaceRef) -> Option<Vec<u8>> {
  if !path.exists() {
    return None;
  }
  match extract_face(path, *index) {
    Ok(font) if covers_hangul(&font) => Some(font),
    Ok(_) => {
      warn!("{} (face {index}) has no Hangul", path.display());
      None
    }
    Err(err) => {
      warn!("Couldn't load {} (face {index}): {err:#}", path.display());
      None
    }
  }
}

/// Copies face `index` of a font file (a `.ttc` collection, or a plain `.ttf`/`.otf` as face 0)
/// into a standalone font, reading only that face's tables. The copy's `head` checksum
/// adjustment goes stale; font parsers, skrifa included, don't check it.
fn extract_face(path: &Path, index: u32) -> anyhow::Result<Vec<u8>> {
  let mut file = FontFile::open(path)?;
  let face_offset = if file.read(0, 4)? == b"ttcf" {
    let count = file.read_u32(8)?;
    ensure!(index < count, "the collection has {count} faces");
    file.read_u32(12 + 4 * u64::from(index))?
  } else {
    ensure!(index == 0, "not a font collection");
    0
  };
  let mut font = file.read(u64::from(face_offset), 12)?;
  let table_count = usize::from(u16::from_be_bytes([font[4], font[5]]));
  let records = file.read(u64::from(face_offset) + 12, 16 * table_count)?;
  font.resize(12 + records.len(), 0);
  for (i, record) in records.as_chunks::<16>().0.iter().enumerate() {
    let table = file.read(u64::from(be_u32(&record[8..])), be_u32(&record[12..]) as usize)?;
    let offset = u32::try_from(font.len())?;
    let entry = &mut font[12 + 16 * i..][..16];
    // Same tag, checksum and length; only the offset moves.
    entry.copy_from_slice(record);
    entry[8..12].copy_from_slice(&offset.to_be_bytes());
    font.extend_from_slice(&table);
    font.resize(font.len().next_multiple_of(4), 0);
  }
  Ok(font)
}

fn covers_hangul(font: &[u8]) -> bool {
  FontRef::new(font).is_ok_and(|font| {
    let charmap = font.charmap();
    PROBE.iter().all(|&c| charmap.map(c).is_some())
  })
}

/// A font file read in ranges, refusing any range past its end.
struct FontFile {
  file: File,
  len: u64,
}

impl FontFile {
  fn open(path: &Path) -> anyhow::Result<Self> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    Ok(Self { file, len })
  }

  fn read(&mut self, offset: u64, len: usize) -> anyhow::Result<Vec<u8>> {
    let end = offset.checked_add(len as u64);
    ensure!(end.is_some_and(|end| end <= self.len), "{len} bytes at {offset} run past the end of the file");
    let mut bytes = vec![0; len];
    self.file.seek(SeekFrom::Start(offset))?;
    self.file.read_exact(&mut bytes)?;
    Ok(bytes)
  }

  fn read_u32(&mut self, offset: u64) -> anyhow::Result<u32> {
    Ok(be_u32(&self.read(offset, 4)?))
  }
}

fn be_u32(bytes: &[u8]) -> u32 {
  u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use tempfile::NamedTempFile;

  use super::*;

  mod extract_face_tests {
    use super::*;

    #[test]
    fn copies_one_face_out_of_a_collection() {
      let file = write(&collection(&[&[("aaaa", "first")], &[("aaaa", "second"), ("bbbb", "other table")]]));
      let font = extract_face(file.path(), 1).unwrap();
      assert_eq!(tables(&font), ["aaaa=second", "bbbb=other table"]);
      assert_eq!(font.len() % 4, 0);
    }

    #[test]
    fn reads_a_plain_font_as_face_zero() {
      let file = write(&standalone(&[("aaaa", "only")]));
      assert_eq!(tables(&extract_face(file.path(), 0).unwrap()), ["aaaa=only"]);
      assert!(extract_face(file.path(), 1).is_err());
    }

    #[test]
    fn rejects_a_face_the_collection_does_not_have() {
      let file = write(&collection(&[&[("aaaa", "first")]]));
      assert!(extract_face(file.path(), 1).is_err());
    }

    #[test]
    fn rejects_a_table_past_the_end_of_the_file() {
      let mut font = standalone(&[("aaaa", "only")]);
      font.truncate(font.len() - 4);
      let file = write(&font);
      assert!(extract_face(file.path(), 0).is_err());
    }
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn loads_apple_sd_gothic_neo_on_macos() {
    let fonts = hangul_fonts().expect("macOS ships Apple SD Gothic Neo");
    assert!(fonts.medium.is_some() && fonts.semibold.is_some());
    for weight in Weight::ALL {
      let face = fonts.face(weight);
      assert!(covers_hangul(face));
      // One face out of the 55 MB collection.
      assert!(face.len() < 8_000_000, "{} bytes", face.len());
    }
  }

  /// A table as (tag, contents).
  type Table<'a> = (&'a str, &'a str);

  fn write(bytes: &[u8]) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file
  }

  /// A plain font: one table directory at the start, then its tables.
  fn standalone(tables: &[Table<'_>]) -> Vec<u8> {
    let mut font = Vec::new();
    append_face(&mut font, tables);
    font
  }

  /// A `.ttc`: the collection header, then each face's directory and tables.
  fn collection(faces: &[&[Table<'_>]]) -> Vec<u8> {
    let mut font = b"ttcf\x00\x01\x00\x00".to_vec();
    font.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    let offsets_at = font.len();
    font.resize(offsets_at + 4 * faces.len(), 0);
    for (i, tables) in faces.iter().enumerate() {
      let offset = font.len() as u32;
      font[offsets_at + 4 * i..][..4].copy_from_slice(&offset.to_be_bytes());
      append_face(&mut font, tables);
    }
    font
  }

  fn append_face(font: &mut Vec<u8>, tables: &[Table<'_>]) {
    let directory_at = font.len();
    font.extend_from_slice(b"OTTO");
    font.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    font.resize(directory_at + 12 + 16 * tables.len(), 0);
    for (i, (tag, contents)) in tables.iter().enumerate() {
      let record_at = directory_at + 12 + 16 * i;
      let offset = font.len() as u32;
      font[record_at..][..4].copy_from_slice(tag.as_bytes());
      font[record_at + 8..][..4].copy_from_slice(&offset.to_be_bytes());
      font[record_at + 12..][..4].copy_from_slice(&(contents.len() as u32).to_be_bytes());
      font.extend_from_slice(contents.as_bytes());
    }
  }

  /// The tables of a standalone font as `tag=contents`, read back through its directory.
  fn tables(font: &[u8]) -> Vec<String> {
    let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
    (0..count)
      .map(|i| {
        let record = &font[12 + 16 * i..][..16];
        let (offset, len) = (be_u32(&record[8..]) as usize, be_u32(&record[12..]) as usize);
        format!("{}={}", String::from_utf8_lossy(&record[..4]), String::from_utf8_lossy(&font[offset..][..len]))
      })
      .collect()
  }
}
