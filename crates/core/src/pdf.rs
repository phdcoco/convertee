//! PPTX를 PDF로 바꾼다.
//!
//! 첫판 방식: betteroffice-pptx가 슬라이드마다 그림(PNG)을 그리고, krilla가 그 그림을
//! PDF 한 쪽에 하나씩 붙인다. 모두 러스트로 돈다.
//! 한계: PDF에서 글자를 선택하거나 검색할 수 없다 (글자도 그림의 일부다).
//!
//! 글꼴: 윈도우 글꼴(맑은 고딕 등)은 맥에 없어서 맥 글꼴로 대신 그린다. betteroffice는 원래
//! 글꼴의 줄 높이 표만 갖고 있고 글자 폭은 대신 그린 글꼴의 것을 쓴다. 폭이 다르면 공백으로
//! 자리를 맞춘 글자가 밀리므로, 대신 쓰는 맥 글꼴의 폭 표를 원래 글꼴의 실제 폭으로 고쳐서
//! 등록한다 (메모리 안에서만 고치고 글꼴 파일은 건드리지 않는다).

use std::fmt;
use std::sync::OnceLock;

use betteroffice_pptx::{Presentation, RenderOptions};
use krilla::Document;
use krilla::geom::Size;
use krilla::image::Image;
use krilla::page::PageSettings;

/// 슬라이드를 그릴 배율. 슬라이드 1인치를 96 x 배율 픽셀로 그린다.
pub const DEFAULT_SCALE: f32 = 3.0;

/// PPTX 파일 한 개의 상한 (압축된 크기)
const MAX_PPTX_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug)]
pub enum PdfError {
    /// PPTX로 읽을 수 없음
    Open(String),
    /// 그릴 글꼴을 이 맥에서 찾지 못함
    Fonts(String),
    /// 슬라이드를 그리지 못함
    Render { slide: usize, reason: String },
    /// PDF를 쓰지 못함
    Write(String),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::Open(e) => write!(f, "PPTX를 읽지 못했습니다: {e}"),
            PdfError::Fonts(e) => write!(f, "글꼴을 준비하지 못했습니다: {e}"),
            PdfError::Render { slide, reason } => write!(f, "{slide}번 슬라이드를 그리지 못했습니다: {reason}"),
            PdfError::Write(e) => write!(f, "PDF를 쓰지 못했습니다: {e}"),
        }
    }
}

impl std::error::Error for PdfError {}

/// 바꾼 결과
#[derive(Debug, Clone)]
pub struct PdfOutput {
    /// PDF 파일 내용
    pub bytes: Vec<u8>,
    /// 쪽 수 (= 슬라이드 수)
    pub pages: usize,
    /// 그리지 못하고 건너뛴 그림 수 (모르는 그림 형식 등)
    pub skipped_images: usize,
}

/// 그릴 때 쓰는 글꼴 한 벌. PPTX가 부르는 이름과 맥 글꼴 파일 내용을 짝지은 것.
pub struct FontSet {
    faces: Vec<FontFace>,
}

struct FontFace {
    family: &'static str,
    bold: bool,
    italic: bool,
    bytes: Vec<u8>,
}

/// 대신 쓰는 맥 글꼴 한 개
struct MacFont {
    /// PPTX에서 부르는 이름
    family: &'static str,
    bold: bool,
    italic: bool,
    /// 글꼴 파일. 이름만 있으면 SUPPLEMENTAL 폴더 안
    path: &'static str,
    /// 묶음 파일(.ttc)이면 그 안에서 고를 글꼴의 PostScript 이름
    postscript: Option<&'static str>,
    /// 원래 글꼴의 글자 폭으로 고칠 범위
    widths: &'static [Width],
}

/// 유니코드 범위의 글자 폭을 em의 1/1000 단위로 정한다.
struct Width {
    from: u32,
    to: u32,
    per_mille: u32,
}

/// 맑은 고딕(보통, 굵게)의 실제 글자 폭. ilovepdf가 만든 PDF에 들어 있는 맑은 고딕의
/// 폭 표(/W)에서 읽고, 같은 PDF의 글자 위치로 확인했다 (18pt 줄에서 공백 0.353~0.360 em).
/// 맥 대신 글꼴(Apple SD 산돌고딕 Neo)은 한글 0.865, 공백 0.263이라 그대로 쓰면 줄이 짧아진다.
const MALGUN_WIDTHS: &[Width] = &[
    Width { from: 0x20, to: 0x20, per_mille: 352 },
    // 한글 음절 전체
    Width { from: 0xAC00, to: 0xD7A3, per_mille: 1000 },
];

const SD_GOTHIC: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";
const SUPPLEMENTAL: &str = "/System/Library/Fonts/Supplemental/";

const fn mac(family: &'static str, bold: bool, italic: bool, path: &'static str) -> MacFont {
    MacFont { family, bold, italic, path, postscript: None, widths: &[] }
}

/// 맨 앞에 등록한 이름이 "모르는 글꼴"을 대신 그리는 기본 글꼴이 된다.
const MAC_FONTS: &[MacFont] = &[
    MacFont {
        family: "맑은 고딕",
        bold: false,
        italic: false,
        path: SD_GOTHIC,
        postscript: Some("AppleSDGothicNeo-Regular"),
        widths: MALGUN_WIDTHS,
    },
    MacFont {
        family: "맑은 고딕",
        bold: true,
        italic: false,
        path: SD_GOTHIC,
        postscript: Some("AppleSDGothicNeo-Bold"),
        widths: MALGUN_WIDTHS,
    },
    // Arial, Times New Roman은 맥에도 같은 글꼴이 있다
    mac("Arial", false, false, "Arial.ttf"),
    mac("Arial", true, false, "Arial Bold.ttf"),
    mac("Arial", false, true, "Arial Italic.ttf"),
    mac("Arial", true, true, "Arial Bold Italic.ttf"),
    mac("Times New Roman", false, false, "Times New Roman.ttf"),
    mac("Times New Roman", true, false, "Times New Roman Bold.ttf"),
    mac("Times New Roman", false, true, "Times New Roman Italic.ttf"),
    mac("Times New Roman", true, true, "Times New Roman Bold Italic.ttf"),
    // 수식 기호(ℝ, ∈ 등)가 많은 글꼴
    mac("Cambria Math", false, false, "Arial Unicode.ttf"),
];

impl FontSet {
    /// 이 맥의 글꼴로 한 벌을 만든다. 없는 파일은 건너뛰고, 하나도 없으면 오류.
    pub fn mac() -> Result<FontSet, PdfError> {
        let mut faces = Vec::new();
        for font in MAC_FONTS {
            let path = if font.path.starts_with('/') {
                font.path.to_owned()
            } else {
                format!("{SUPPLEMENTAL}{}", font.path)
            };
            let Ok(data) = std::fs::read(&path) else { continue };
            let mut bytes = match font.postscript {
                Some(name) => match face_from_collection(&data, name) {
                    Some(b) => b,
                    None => continue,
                },
                None => data,
            };
            if !font.widths.is_empty() && set_advances(&mut bytes, font.widths).is_none() {
                // 폭을 고칠 수 없으면 줄이 어긋나므로 이 글꼴은 쓰지 않는다
                continue;
            }
            faces.push(FontFace { family: font.family, bold: font.bold, italic: font.italic, bytes });
        }
        if faces.is_empty() {
            return Err(PdfError::Fonts("맥 기본 글꼴 파일을 하나도 찾지 못했습니다".into()));
        }
        Ok(FontSet { faces })
    }

    /// 앱이 켜져 있는 동안 한 번만 읽어 둔다.
    pub fn mac_shared() -> Result<&'static FontSet, PdfError> {
        static SET: OnceLock<Result<FontSet, String>> = OnceLock::new();
        SET.get_or_init(|| FontSet::mac().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| PdfError::Fonts(e.clone()))
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

/// PPTX 파일 내용을 받아 PDF 파일 내용을 돌려준다. 맥 글꼴과 기본 배율을 쓴다.
///
/// 슬라이드 그림은 손실 없는 PNG 그대로 담는다. 사람이 차이를 못 느끼는 JPEG(butteraugli 1.0
/// 이하)로 담는 방식도 재 보았으나 강의 자료 40쪽에서 크기가 2~8%만 줄고 시간은 40배 넘게
/// 들어서 쓰지 않는다 (글자 가장자리 때문에 화질을 거의 내릴 수 없다).
pub fn pptx_to_pdf(pptx: &[u8]) -> Result<PdfOutput, PdfError> {
    pptx_to_pdf_with(pptx, FontSet::mac_shared()?, DEFAULT_SCALE)
}

/// 글꼴과 배율을 정해서 바꾼다.
pub fn pptx_to_pdf_with(pptx: &[u8], fonts: &FontSet, scale: f32) -> Result<PdfOutput, PdfError> {
    if pptx.len() > MAX_PPTX_BYTES {
        return Err(PdfError::Open("파일이 너무 큽니다".into()));
    }
    let unwrapped = unwrap_bitmap_metafiles(pptx);
    let pptx = unwrapped.as_deref().unwrap_or(pptx);
    let mut deck = Presentation::open(pptx).map_err(|e| PdfError::Open(e.to_string()))?;
    for face in &fonts.faces {
        deck.register_font(face.family, face.bold, face.italic, &face.bytes)
            .map_err(|e| PdfError::Fonts(format!("{}: {e}", face.family)))?;
    }

    let options = RenderOptions { scale, ..RenderOptions::default() };
    let mut document = Document::new();
    let mut skipped_images = 0;
    let pages = deck.slides().len();
    for index in 0..pages {
        let slide = deck
            .render_png(index, &options)
            .map_err(|e| PdfError::Render { slide: index + 1, reason: e.to_string() })?;
        skipped_images += slide.skipped_images;

        // 그림은 CSS 픽셀(1인치 = 96) x 배율. PDF는 포인트(1인치 = 72).
        let width_pt = slide.width as f32 / scale * 0.75;
        let height_pt = slide.height as f32 / scale * 0.75;
        let size = Size::from_wh(width_pt, height_pt)
            .ok_or_else(|| PdfError::Render { slide: index + 1, reason: "슬라이드 크기가 0입니다".into() })?;
        let image = Image::from_png(slide.bytes.into(), true)
            .map_err(|e| PdfError::Write(format!("{}번 슬라이드 그림: {e}", index + 1)))?;

        let mut page = document.start_page_with(PageSettings::new(size));
        let mut surface = page.surface();
        surface.draw_image(image, size);
        surface.finish();
        page.finish();
    }
    let bytes = document.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    Ok(PdfOutput { bytes, pages, skipped_images })
}

/// 한 EMF 그림 파일의 상한. betteroffice가 한 발표 자료에서 EMF를 읽는 한도와 같다.
const MAX_METAFILE_BYTES: u64 = 32 * 1024 * 1024;

/// 비트맵 한 장만 감싼 EMF/WMF 그림을 PNG로 풀어 넣은 PPTX를 만든다. 바꿀 것이 없으면 None.
///
/// betteroffice는 EMF를 선과 도형으로만 그리고, 안에 비트맵이 든 EMF는 그리지 않고 건너뛴다.
/// 스캔한 그림이나 다른 프로그램에서 붙여 넣은 그림이 이런 EMF로 들어오는 일이 많아서,
/// 그 비트맵을 꺼내 같은 이름의 PNG 내용으로 바꿔 둔다 (그리는 쪽은 내용을 보고 형식을 안다).
/// 선과 도형이 섞인 EMF는 건드리지 않는다.
fn unwrap_bitmap_metafiles(pptx: &[u8]) -> Option<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    use zip::write::SimpleFileOptions;

    let mut archive = zip::ZipArchive::new(Cursor::new(pptx)).ok()?;
    let mut replaced: Vec<Option<Vec<u8>>> = vec![None; archive.len()];
    for (i, slot) in replaced.iter_mut().enumerate() {
        let Ok(mut file) = archive.by_index(i) else { continue };
        if !file.name().starts_with("ppt/media/") || file.size() > MAX_METAFILE_BYTES {
            continue;
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        if file.by_ref().take(MAX_METAFILE_BYTES).read_to_end(&mut bytes).is_err() {
            continue;
        }
        *slot = single_bitmap_png(&bytes);
    }
    if replaced.iter().all(Option::is_none) {
        return None;
    }

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(pptx.len())));
    for (i, png) in replaced.into_iter().enumerate() {
        match png {
            None => writer.raw_copy_file(archive.by_index_raw(i).ok()?).ok()?,
            Some(png) => {
                let name = archive.by_index_raw(i).ok()?.name().to_owned();
                // PNG는 이미 압축되어 있어 그대로 담는다
                let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
                writer.start_file(name, options).ok()?;
                writer.write_all(&png).ok()?;
            }
        }
    }
    Some(writer.finish().ok()?.into_inner())
}

/// EMF/WMF가 그림 전체를 덮는 비트맵 한 장뿐이면 그 비트맵을 PNG로 돌려준다.
fn single_bitmap_png(bytes: &[u8]) -> Option<Vec<u8>> {
    use ooxml_metafile::drawing::{Op, Pixels};

    if !ooxml_metafile::is_metafile(bytes) || ooxml_metafile::decode(bytes).is_some() {
        // 메타파일이 아니거나, betteroffice가 선과 도형으로 직접 그릴 수 있다
        return None;
    }
    let drawing = ooxml_metafile::replay(bytes).ok()?;
    let [Op::Image(image)] = drawing.ops.as_slice() else { return None };
    if image.opacity < 1.0 || image.clip.is_some() || !drawing.omissions.is_empty() {
        return None;
    }
    // 회전, 기울임 없이 그림 틀 전체를 덮어야 한다. GDI 반올림과 경계 포함 방식 때문에
    // 틀의 2%에 비트맵 1픽셀을 더한 만큼까지는 어긋날 수 있다 (실제 예: 49줄 비트맵을 48줄 남짓한 틀에 그린 EMF).
    let [sx, ky, kx, sy, tx, ty] = image.transform;
    let (w, h) = (image.bitmap.width as f64, image.bitmap.height as f64);
    let close = |a: f64, b: f64, pixel: f64| (a - b).abs() <= 0.02 * b.abs() + pixel;
    let covers = ky == 0.0
        && kx == 0.0
        && sx > 0.0
        && sy > 0.0
        && close(tx, 0.0, sx)
        && close(ty, 0.0, sy)
        && close(sx * w, drawing.width, sx)
        && close(sy * h, drawing.height, sy);
    if !covers {
        return None;
    }
    match &image.bitmap.pixels {
        Pixels::Encoded { mime: "image/png" | "image/jpeg", bytes } => Some(bytes.clone()),
        Pixels::Encoded { .. } => None,
        Pixels::Rgba(rgba) => {
            use image::ImageEncoder as _;
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(rgba, image.bitmap.width, image.bitmap.height, image::ExtendedColorType::Rgba8)
                .ok()?;
            Some(png)
        }
    }
}

/// 단독 글꼴 파일의 글자 폭(hmtx 표)을 바꾼다. 글자 모양은 그대로 두어 왼쪽에 붙는다.
/// 고친 글자 수를 돌려주고, 표를 읽을 수 없으면 None.
fn set_advances(font: &mut [u8], widths: &[Width]) -> Option<usize> {
    let units_per_em = be16(font, table_offset(font, b"head")? + 18)? as u32;
    let metrics = be16(font, table_offset(font, b"hhea")? + 34)? as u32;
    let hmtx = table_offset(font, b"hmtx")?;
    let cmap = Cmap::find(font, table_offset(font, b"cmap")?)?;
    let mut changed = 0;
    for width in widths {
        let advance = u16::try_from(width.per_mille * units_per_em / 1000).ok()?;
        for code in width.from..=width.to {
            let Some(glyph) = cmap.glyph(font, code) else { continue };
            if glyph == 0 || glyph >= metrics {
                // 없는 글자이거나, 표 끝에서 폭을 함께 쓰는 글자라 따로 고칠 수 없다
                continue;
            }
            let at = hmtx + 4 * glyph as usize;
            font.get_mut(at..at + 2)?.copy_from_slice(&advance.to_be_bytes());
            changed += 1;
        }
    }
    Some(changed)
}

/// 단독 글꼴 파일에서 표 하나가 시작하는 위치
fn table_offset(font: &[u8], tag: &[u8; 4]) -> Option<usize> {
    let num_tables = be16(font, 4)? as usize;
    (0..num_tables).find_map(|i| {
        let r = 12 + 16 * i;
        (font.get(r..r + 4)? == tag).then(|| be32(font, r + 8).map(|o| o as usize))?
    })
}

/// 글꼴의 글자 번호 표(cmap). 유니코드 전체(형식 12)를 먼저, 없으면 기본 평면(형식 4)을 쓴다.
enum Cmap {
    Format4(usize),
    Format12(usize),
}

impl Cmap {
    fn find(font: &[u8], cmap: usize) -> Option<Cmap> {
        let mut found = None;
        for i in 0..be16(font, cmap + 2)? as usize {
            let r = cmap + 4 + 8 * i;
            let (platform, encoding) = (be16(font, r)?, be16(font, r + 2)?);
            let unicode = platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10));
            let sub = cmap + be32(font, r + 4)? as usize;
            match be16(font, sub)? {
                12 if unicode => return Some(Cmap::Format12(sub)),
                4 if unicode => found = Some(Cmap::Format4(sub)),
                _ => {}
            }
        }
        found
    }

    fn glyph(&self, font: &[u8], code: u32) -> Option<u32> {
        match *self {
            // 두 형식 모두 구간이 끝 글자 순으로 놓여 있어 이진 탐색으로 찾는다
            Cmap::Format12(sub) => {
                let groups = be32(font, sub + 12)? as usize;
                let end = |i: usize| be32(font, sub + 16 + 12 * i + 4).unwrap_or(u32::MAX);
                let i = first_not_below(groups, |i| end(i) < code);
                let g = sub + 16 + 12 * i;
                let start = be32(font, g)?;
                (i < groups && start <= code).then(|| be32(font, g + 8).map(|first| first + code - start))?
            }
            Cmap::Format4(sub) => {
                let code = u16::try_from(code).ok()?;
                let segments = be16(font, sub + 6)? as usize / 2;
                let end = |i: usize| be16(font, sub + 14 + 2 * i).unwrap_or(u16::MAX);
                // 형식 4는 마지막 구간이 0xFFFF로 끝나야 하므로 언제나 한 구간이 걸린다
                if let Some(i) = Some(first_not_below(segments, |i| end(i) < code)).filter(|&i| i < segments) {
                    let start = be16(font, sub + 16 + 2 * segments + 2 * i)?;
                    if code < start {
                        return None;
                    }
                    let delta = be16(font, sub + 16 + 4 * segments + 2 * i)?;
                    let range_at = sub + 16 + 6 * segments + 2 * i;
                    let range = be16(font, range_at)? as usize;
                    let glyph = if range == 0 {
                        code.wrapping_add(delta)
                    } else {
                        match be16(font, range_at + range + 2 * (code - start) as usize)? {
                            0 => 0,
                            g => g.wrapping_add(delta),
                        }
                    };
                    return Some(glyph as u32);
                }
                None
            }
        }
    }
}

/// 0..len 중 below가 처음으로 거짓인 번호 (모두 참이면 len). below는 앞쪽에서만 참이어야 한다.
fn first_not_below(len: usize, below: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, len);
    while low < high {
        let mid = low + (high - low) / 2;
        if below(mid) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

/// 글꼴 묶음 파일(.ttc)에서 PostScript 이름이 같은 글꼴 하나를 꺼내 단독 글꼴 파일로 만든다.
/// 묶음이 아니면 None.
pub fn face_from_collection(data: &[u8], postscript: &str) -> Option<Vec<u8>> {
    if data.get(0..4)? != b"ttcf" {
        return None;
    }
    let count = be32(data, 8)? as usize;
    (0..count.min(256))
        .filter_map(|i| be32(data, 12 + 4 * i).map(|o| o as usize))
        .find(|&offset| postscript_name(data, offset).as_deref() == Some(postscript))
        .and_then(|offset| standalone_face(data, offset))
}

/// 묶음 안 offset에서 시작하는 글꼴의 표들을 모아 단독 글꼴 파일을 만든다.
fn standalone_face(data: &[u8], offset: usize) -> Option<Vec<u8>> {
    let num_tables = be16(data, offset + 4)? as usize;
    let mut records = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let r = offset + 12 + 16 * i;
        let tag = data.get(r..r + 4)?;
        let checksum = be32(data, r + 4)?;
        let table_offset = be32(data, r + 8)? as usize;
        let length = be32(data, r + 12)? as usize;
        let table = data.get(table_offset..table_offset.checked_add(length)?)?;
        records.push((tag, checksum, table));
    }

    let entry_selector = (num_tables.max(1) as f64).log2().floor() as u16;
    let search_range = 16u16.checked_mul(1 << entry_selector)?;
    let range_shift = (num_tables as u16).checked_mul(16)?.saturating_sub(search_range);

    let mut out = Vec::new();
    out.extend_from_slice(data.get(offset..offset + 4)?); // sfnt 버전
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());
    let mut next = 12 + 16 * num_tables;
    for (tag, checksum, table) in &records {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(next as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        next += table.len().div_ceil(4) * 4;
    }
    for (_, _, table) in &records {
        out.extend_from_slice(table);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    Some(out)
}

/// 글꼴의 name 표에서 PostScript 이름(이름 번호 6)을 읽는다.
fn postscript_name(data: &[u8], offset: usize) -> Option<String> {
    let num_tables = be16(data, offset + 4)? as usize;
    let name_table = (0..num_tables).find_map(|i| {
        let r = offset + 12 + 16 * i;
        (data.get(r..r + 4)? == b"name").then(|| be32(data, r + 8).map(|o| o as usize))?
    })?;
    let count = be16(data, name_table + 2)? as usize;
    let strings = name_table + be16(data, name_table + 4)? as usize;
    for i in 0..count {
        let r = name_table + 6 + 12 * i;
        let platform = be16(data, r)?;
        let name_id = be16(data, r + 6)?;
        if name_id != 6 {
            continue;
        }
        let length = be16(data, r + 8)? as usize;
        let start = strings + be16(data, r + 10)? as usize;
        let raw = data.get(start..start + length)?;
        return Some(match platform {
            // 윈도우, 유니코드: UTF-16BE
            0 | 3 => {
                let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|&c| u16::from_be_bytes(c)).collect();
                String::from_utf16_lossy(&units)
            }
            // 맥: PostScript 이름은 ASCII
            _ => String::from_utf8_lossy(raw).into_owned(),
        });
    }
    None
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
        std::fs::read(&path).unwrap_or_else(|_| {
            panic!("{} 가 없습니다. 먼저 scripts/make-fixtures.sh 를 실행하세요", path.display())
        })
    }

    #[test]
    fn 맥_글꼴_묶음에서_한_글꼴을_꺼낸다() {
        let ttc = std::fs::read(SD_GOTHIC).unwrap();
        let regular = face_from_collection(&ttc, "AppleSDGothicNeo-Regular").expect("Regular를 찾아야 한다");
        let bold = face_from_collection(&ttc, "AppleSDGothicNeo-Bold").expect("Bold를 찾아야 한다");
        assert!(regular.len() < ttc.len());
        assert_ne!(regular, bold);
        // 꺼낸 글꼴은 단독 글꼴로 다시 읽혀야 한다
        assert_eq!(postscript_name(&regular, 0).as_deref(), Some("AppleSDGothicNeo-Regular"));
        assert!(face_from_collection(&ttc, "없는-글꼴").is_none());
        assert!(face_from_collection(b"not a font", "x").is_none());
    }

    /// 단독 글꼴에서 한 글자의 폭 (em의 1/1000)
    fn advance(font: &[u8], code: u32) -> u32 {
        let hmtx = table_offset(font, b"hmtx").unwrap();
        let units_per_em = be16(font, table_offset(font, b"head").unwrap() + 18).unwrap() as u32;
        let cmap = Cmap::find(font, table_offset(font, b"cmap").unwrap()).unwrap();
        let glyph = cmap.glyph(font, code).unwrap() as usize;
        be16(font, hmtx + 4 * glyph).unwrap() as u32 * 1000 / units_per_em
    }

    #[test]
    fn 맑은_고딕_대신_쓰는_글꼴의_폭을_맑은_고딕에_맞춘다() {
        let ttc = std::fs::read(SD_GOTHIC).unwrap();
        let mut face = face_from_collection(&ttc, "AppleSDGothicNeo-Regular").unwrap();
        // 고치기 전: 이 맥의 글꼴 파일에서 잰 값
        assert_eq!(advance(&face, 0xAC00), 865);
        assert_eq!(advance(&face, 0x20), 263);
        let changed = set_advances(&mut face, MALGUN_WIDTHS).unwrap();
        assert_eq!(changed, 1 + 11172, "공백 1자와 한글 음절 11172자를 모두 고쳐야 한다");
        assert_eq!(advance(&face, 0xAC00), 1000);
        assert_eq!(advance(&face, 0xD7A3), 1000);
        assert_eq!(advance(&face, 0x20), 352);
        // 라틴 글자는 그대로
        assert_eq!(advance(&face, 0x41), 605);
    }

    #[test]
    fn 맥_글꼴_한_벌을_준비한다() {
        let fonts = FontSet::mac().unwrap();
        assert_eq!(fonts.len(), MAC_FONTS.len(), "이 맥에 있어야 할 글꼴 파일이 빠졌다");
    }

    #[test]
    fn 실제_강의_pptx를_슬라이드마다_한_쪽인_pdf로_바꾼다() {
        let pptx = fixture("lecture.pptx");
        let out = pptx_to_pdf(&pptx).unwrap();
        assert!(out.bytes.starts_with(b"%PDF-"));
        assert_eq!(out.pages, 40);
        assert_eq!(out.skipped_images, 0, "그리지 못한 그림이 있다");
        eprintln!(
            "lecture.pptx {} 바이트 -> PDF {} 바이트, {}쪽, 건너뛴 그림 {}",
            pptx.len(),
            out.bytes.len(),
            out.pages,
            out.skipped_images
        );
    }

    #[test]
    fn pptx가_아니면_읽기_오류를_낸다() {
        let fonts = FontSet::mac().unwrap();
        assert!(matches!(pptx_to_pdf_with(b"not a pptx", &fonts, 1.0), Err(PdfError::Open(_))));
    }
}
