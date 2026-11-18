//! 핵심 계산 한 번에 걸리는 시간을 잰다. 파일 종류는 내용을 보고 정한다.
//! 쓰는 법: cargo run --release -p convertee-core --example measure -- <PNG 또는 PPTX 파일>...
//!
//! PPTX는 환경 변수로 방식을 바꿔 잴 수 있다.
//!   CONVERTEE_SCALE=2      슬라이드 배율 (기본: 앱과 같음)
//!   CONVERTEE_OUT=폴더      만든 PDF를 그 폴더에 쓰기

use std::time::Instant;

use convertee_core::{FileKind, FontSet, detect, pptx_to_pdf_with};

fn main() {
    for path in std::env::args().skip(1) {
        let input = std::fs::read(&path).expect("파일을 읽지 못했습니다");
        match detect(&input) {
            FileKind::Png => png(&path, &input),
            FileKind::Pptx => pptx(&path, &input),
            other => println!("{path}: {} 는 잴 수 없습니다", other.label()),
        }
    }
}

fn png(path: &str, input: &[u8]) {
    let started = Instant::now();
    let out = convertee_core::png_to_jpeg(input).expect("바꾸지 못했습니다");
    let ms = started.elapsed().as_millis();
    println!(
        "{path}: {}x{}, PNG {} 바이트 -> JPEG {} 바이트, 화질 {}, 거리 {:?}, {ms} ms",
        out.width,
        out.height,
        input.len(),
        out.bytes.len(),
        out.quality,
        out.distance
    );
}

fn pptx(path: &str, input: &[u8]) {
    let scale = std::env::var("CONVERTEE_SCALE")
        .ok()
        .map(|s| s.parse().expect("CONVERTEE_SCALE는 숫자여야 합니다"))
        .unwrap_or(convertee_core::DEFAULT_SCALE);
    let fonts = FontSet::mac_shared().expect("글꼴");
    let started = Instant::now();
    let out = pptx_to_pdf_with(input, fonts, scale).expect("바꾸지 못했습니다");
    let ms = started.elapsed().as_millis();
    println!(
        "{path}: 배율 {scale}, PPTX {} 바이트 -> PDF {} 바이트, {}쪽, 건너뛴 그림 {}, {ms} ms",
        input.len(),
        out.bytes.len(),
        out.pages,
        out.skipped_images
    );
    if let Ok(dir) = std::env::var("CONVERTEE_OUT") {
        let name = format!("scale{scale}.pdf");
        std::fs::write(std::path::Path::new(&dir).join(&name), &out.bytes).expect("PDF를 쓰지 못했습니다");
    }
}
