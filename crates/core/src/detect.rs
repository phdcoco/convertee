//! 파일 앞부분의 바이트를 읽어 종류를 알아낸다. 확장자는 믿지 않는다.

/// 앱이 알아보는 파일 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Png,
    Jpeg,
    Heic,
    Gif,
    Webp,
    Bmp,
    Tiff,
    Pdf,
    Pptx,
    Docx,
    Xlsx,
    /// 압축 파일이지만 위 문서 종류로는 확인되지 않음
    Zip,
    Unknown,
}

/// 바꿀 수 있는 결과 형식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Jpeg,
}

impl FileKind {
    /// 화면에 보여 줄 이름
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Png => "PNG 이미지",
            FileKind::Jpeg => "JPEG 이미지",
            FileKind::Heic => "HEIC 이미지",
            FileKind::Gif => "GIF 이미지",
            FileKind::Webp => "WebP 이미지",
            FileKind::Bmp => "BMP 이미지",
            FileKind::Tiff => "TIFF 이미지",
            FileKind::Pdf => "PDF 문서",
            FileKind::Pptx => "파워포인트(PPTX)",
            FileKind::Docx => "워드(DOCX)",
            FileKind::Xlsx => "엑셀(XLSX)",
            FileKind::Zip => "ZIP 압축 파일",
            FileKind::Unknown => "알 수 없는 파일",
        }
    }

    /// 지금 판에서 바꿀 수 있는 형식
    pub fn targets(self) -> &'static [Target] {
        match self {
            FileKind::Png => &[Target::Jpeg],
            _ => &[],
        }
    }
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Target::Jpeg => "JPEG",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Target::Jpeg => "jpg",
        }
    }
}

/// 종류를 알아내려고 읽는 앞부분 길이. 문서 압축 파일은 안쪽 파일 이름을 찾으려고 넉넉히 읽는다.
pub const HEAD_LEN: usize = 64 * 1024;

/// 파일 앞부분(최대 HEAD_LEN 바이트)으로 종류를 알아낸다.
pub fn detect(head: &[u8]) -> FileKind {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return FileKind::Png;
    }
    if head.starts_with(b"\xff\xd8\xff") {
        return FileKind::Jpeg;
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return FileKind::Gif;
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return FileKind::Webp;
    }
    if head.starts_with(b"BM") && head.len() >= 26 {
        return FileKind::Bmp;
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return FileKind::Tiff;
    }
    if head.starts_with(b"%PDF-") {
        return FileKind::Pdf;
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        let heic_brands: [&[u8]; 7] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"mif1", b"msf1"];
        if heic_brands.contains(&brand) {
            return FileKind::Heic;
        }
    }
    if head.starts_with(b"PK\x03\x04") {
        // 압축 안쪽 파일 이름은 압축되지 않은 채 들어 있어 그대로 찾을 수 있다
        if contains(head, b"ppt/") {
            return FileKind::Pptx;
        }
        if contains(head, b"word/") {
            return FileKind::Docx;
        }
        if contains(head, b"xl/") {
            return FileKind::Xlsx;
        }
        return FileKind::Zip;
    }
    FileKind::Unknown
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 확장자가_아니라_내용으로_알아본다() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), FileKind::Png);
        assert_eq!(detect(b"\xff\xd8\xff\xe0\0\x10JFIF"), FileKind::Jpeg);
        assert_eq!(detect(b"%PDF-1.7\n"), FileKind::Pdf);
        assert_eq!(detect(b"\0\0\0\x18ftypheic\0\0\0\0"), FileKind::Heic);
        assert_eq!(detect(b"RIFF\0\0\0\0WEBPVP8 "), FileKind::Webp);
        assert_eq!(detect(b"hello"), FileKind::Unknown);
        assert_eq!(detect(b""), FileKind::Unknown);
    }

    #[test]
    fn 문서_압축_파일을_구분한다() {
        let mut pptx = b"PK\x03\x04".to_vec();
        pptx.extend_from_slice(b"....[Content_Types].xml....ppt/presentation.xml");
        assert_eq!(detect(&pptx), FileKind::Pptx);
        assert_eq!(detect(b"PK\x03\x04....word/document.xml"), FileKind::Docx);
        assert_eq!(detect(b"PK\x03\x04....other.txt"), FileKind::Zip);
    }

    #[test]
    fn 지금은_png만_jpeg로_바꿀_수_있다() {
        assert_eq!(FileKind::Png.targets(), &[Target::Jpeg]);
        assert!(FileKind::Pdf.targets().is_empty());
    }
}
