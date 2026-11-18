//! convertee의 핵심 계산. 모두 러스트로 돌고 별도 프로그램을 켜지 않는다.

pub mod detect;
pub mod jpeg;
pub mod pdf;

pub use detect::{FileKind, HEAD_LEN, Target, detect};
pub use jpeg::{ConvertError, JpegOutput, MAX_DISTANCE, png_to_jpeg};
pub use pdf::{DEFAULT_SCALE, FontSet, PdfError, PdfOutput, pptx_to_pdf, pptx_to_pdf_with};
