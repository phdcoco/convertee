//! PNG를 JPEG로 바꾼다. 사람이 원본과 차이를 느낄 수 없는 가장 낮은 화질을 스스로 찾는다.

use std::fmt;
use std::io::Cursor;

use butteraugli::{ButteraugliParams, ButteraugliReference};
use image::codecs::png::PngDecoder;
use image::{DynamicImage, ImageDecoder, ImageFormat, Limits};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

/// 이 값 이하면 사람이 원본과 같은 그림으로 본다 (butteraugli 거리 기준).
pub const MAX_DISTANCE: f64 = 1.0;

/// JPEG가 담을 수 있는 가장 긴 변
const JPEG_MAX_SIDE: u32 = u16::MAX as u32;

/// butteraugli가 잴 수 있는 가장 짧은 변
const MEASURE_MIN_SIDE: u32 = 8;

/// 읽을 때 쓸 수 있는 메모리 상한. 작은 파일이 거대한 그림으로 풀리는 공격을 막는다.
const DECODE_MAX_ALLOC: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum ConvertError {
    /// PNG로 읽을 수 없음
    Decode(String),
    /// JPEG가 담을 수 없는 크기
    TooLarge { width: u32, height: u32 },
    /// JPEG 쓰기 실패
    Encode(String),
    /// 화질 측정 실패
    Measure(String),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::Decode(e) => write!(f, "PNG를 읽지 못했습니다: {e}"),
            ConvertError::TooLarge { width, height } => write!(
                f,
                "그림이 너무 큽니다 ({width}x{height}). JPEG는 한 변이 {JPEG_MAX_SIDE}픽셀까지입니다"
            ),
            ConvertError::Encode(e) => write!(f, "JPEG를 쓰지 못했습니다: {e}"),
            ConvertError::Measure(e) => write!(f, "화질을 재지 못했습니다: {e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// 바꾼 결과
#[derive(Debug, Clone)]
pub struct JpegOutput {
    /// JPEG 파일 내용
    pub bytes: Vec<u8>,
    /// 고른 화질 (1에서 100)
    pub quality: u8,
    /// 원본과의 차이 (butteraugli 거리). 그림이 너무 작아 재지 못하면 None
    pub distance: Option<f64>,
    pub width: u32,
    pub height: u32,
    /// 원본에 투명한 부분이 있었는지 (있었다면 검은색으로 채움)
    pub had_alpha: bool,
}

/// PNG 파일 내용을 받아 JPEG 파일 내용을 돌려준다.
///
/// 화질은 원본과의 butteraugli 거리가 MAX_DISTANCE 이하인 가장 낮은 값을 고른다.
/// 화질 100에서도 기준을 넘으면 100으로 저장하고 그때의 거리를 돌려준다.
/// 투명한 부분은 검은색으로 채운다.
pub fn png_to_jpeg(png: &[u8]) -> Result<JpegOutput, ConvertError> {
    let mut limits = Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    let mut decoder = PngDecoder::with_limits(Cursor::new(png), limits)
        .map_err(|e| ConvertError::Decode(e.to_string()))?;
    let icc = decoder.icc_profile().ok().flatten();
    let image = DynamicImage::from_decoder(decoder).map_err(|e| ConvertError::Decode(e.to_string()))?;

    let (width, height) = (image.width(), image.height());
    if width > JPEG_MAX_SIDE || height > JPEG_MAX_SIDE {
        return Err(ConvertError::TooLarge { width, height });
    }
    let had_alpha = image.color().has_alpha();
    let rgb = flatten_on_black(&image);

    // 너무 작아 잴 수 없으면 가장 높은 화질로 저장한다
    if width < MEASURE_MIN_SIDE || height < MEASURE_MIN_SIDE {
        let bytes = encode(&rgb, width, height, 100, icc.as_deref())?;
        return Ok(JpegOutput { bytes, quality: 100, distance: None, width, height, had_alpha });
    }

    let reference = ButteraugliReference::new(&rgb, width as usize, height as usize, ButteraugliParams::default())
        .map_err(|e| ConvertError::Measure(e.to_string()))?;
    let measure = |quality: u8| -> Result<(Vec<u8>, f64), ConvertError> {
        let bytes = encode(&rgb, width, height, quality, icc.as_deref())?;
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
            .map_err(|e| ConvertError::Measure(e.to_string()))?
            .to_rgb8();
        let distance = reference
            .compare(decoded.as_raw())
            .map_err(|e| ConvertError::Measure(e.to_string()))?
            .score;
        Ok((bytes, distance))
    };

    // 화질 100부터 확인한다. 여기서도 기준을 넘으면 더 올릴 수 없으니 100으로 저장한다.
    let (best_bytes, best_distance) = measure(100)?;
    let mut best = (100u8, best_bytes, best_distance);
    if best_distance > MAX_DISTANCE {
        let (quality, bytes, distance) = best;
        return Ok(JpegOutput { bytes, quality, distance: Some(distance), width, height, had_alpha });
    }

    // 기준을 지키는 가장 낮은 화질을 반으로 나눠 찾는다. hi는 늘 기준을 지킨다.
    let (mut lo, mut hi) = (1u8, 100u8);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let (bytes, distance) = measure(mid)?;
        if distance <= MAX_DISTANCE {
            hi = mid;
            best = (mid, bytes, distance);
        } else {
            lo = mid + 1;
        }
    }

    let (quality, bytes, distance) = best;
    Ok(JpegOutput { bytes, quality, distance: Some(distance), width, height, had_alpha })
}

/// 투명도를 검은 바탕 위에 합쳐 RGB만 남긴다.
fn flatten_on_black(image: &DynamicImage) -> Vec<u8> {
    if !image.color().has_alpha() {
        return image.to_rgb8().into_raw();
    }
    let rgba = image.to_rgba8();
    let mut rgb = Vec::with_capacity(rgba.width() as usize * rgba.height() as usize * 3);
    for px in rgba.pixels() {
        let a = u16::from(px[3]);
        for c in &px.0[..3] {
            // 검은색(0) 바탕과 섞는다: c * a / 255, 반올림
            rgb.push(((u16::from(*c) * a + 127) / 255) as u8);
        }
    }
    rgb
}

fn encode(rgb: &[u8], width: u32, height: u32, quality: u8, icc: Option<&[u8]>) -> Result<Vec<u8>, ConvertError> {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, quality);
    // 색을 줄여 담지 않는다. 글자나 선 둘레에 색이 번지는 것을 막는다.
    encoder.set_sampling_factor(SamplingFactor::R_4_4_4);
    encoder.set_optimized_huffman_tables(true);
    if let Some(icc) = icc {
        encoder.add_icc_profile(icc).map_err(|e| ConvertError::Encode(e.to_string()))?;
    }
    encoder
        .encode(rgb, width as u16, height as u16, ColorType::Rgb)
        .map_err(|e| ConvertError::Encode(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{FileKind, detect};
    use std::path::PathBuf;

    /// 실제 PNG 예시. scripts/make-fixtures.sh가 이 맥의 사진과 아이콘으로 만든다.
    fn fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
        std::fs::read(&path).unwrap_or_else(|_| {
            panic!("{} 가 없습니다. 먼저 scripts/make-fixtures.sh 를 실행하세요", path.display())
        })
    }

    fn png_bytes(img: &DynamicImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn 실제_사진_png를_차이_없는_jpeg로_바꾼다() {
        let png = fixture("photo.png");
        let out = png_to_jpeg(&png).unwrap();

        assert_eq!(detect(&out.bytes), FileKind::Jpeg);
        let back = image::load_from_memory_with_format(&out.bytes, ImageFormat::Jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (out.width, out.height));
        let distance = out.distance.unwrap();
        assert!(distance <= MAX_DISTANCE, "거리 {distance}");
        // 화질 100보다 낮은 값을 찾아야 의미가 있다
        assert!(out.quality < 100, "화질 {}", out.quality);
        eprintln!(
            "photo.png {} 바이트 -> JPEG {} 바이트, 화질 {}, 거리 {:.3}",
            png.len(),
            out.bytes.len(),
            out.quality,
            distance
        );
    }

    #[test]
    fn 실제_아이콘의_투명한_부분은_검은색이_된다() {
        let png = fixture("transparent.png");
        let source = image::load_from_memory(&png).unwrap().to_rgba8();
        let (x, y, _) = source
            .enumerate_pixels()
            .find(|(_, _, p)| p[3] == 0)
            .expect("예시 아이콘에 완전히 투명한 픽셀이 있어야 한다");

        let out = png_to_jpeg(&png).unwrap();
        assert!(out.had_alpha);
        let back = image::load_from_memory(&out.bytes).unwrap().to_rgb8();
        let p = back.get_pixel(x, y);
        // JPEG는 손실 압축이라 0에서 조금 벗어날 수 있다
        assert!(p.0.iter().all(|&c| c <= 8), "({x},{y}) 픽셀이 {:?}", p.0);
    }

    #[test]
    fn 반투명은_검은_바탕과_섞는다() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([200, 100, 50, 128])));
        assert_eq!(flatten_on_black(&img), vec![100, 50, 25]);
    }

    #[test]
    fn 너무_작은_그림은_가장_높은_화질로_저장한다() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30])));
        let out = png_to_jpeg(&png_bytes(&img)).unwrap();
        assert_eq!(out.quality, 100);
        assert!(out.distance.is_none());
    }

    #[test]
    fn png가_아니면_읽기_오류를_낸다() {
        assert!(matches!(png_to_jpeg(b"not a png"), Err(ConvertError::Decode(_))));
    }
}
