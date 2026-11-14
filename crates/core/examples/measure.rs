//! 핵심 계산 한 번에 걸리는 시간을 잰다.
//! 쓰는 법: cargo run --release -p convertee-core --example measure -- <PNG 파일>...

use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let input = std::fs::read(&path).expect("파일을 읽지 못했습니다");
        let started = Instant::now();
        let out = convertee_core::png_to_jpeg(&input).expect("바꾸지 못했습니다");
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
}
