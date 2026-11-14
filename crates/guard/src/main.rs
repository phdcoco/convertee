//! 릴리스 빌드 뒤에 앱 번들을 검사한다. scripts/release.sh가 부른다.
//!
//! 쓰는 법: convertee-guard bundle <앱.app 경로> [실행 파일 이름]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some("bundle"), Some(app)) = (args.first().map(String::as_str), args.get(1)) else {
        eprintln!("쓰는 법: convertee-guard bundle <앱.app 경로> [실행 파일 이름]");
        return ExitCode::from(2);
    };
    let main_exe = args.get(2).map(String::as_str).unwrap_or("convertee");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let app = Path::new(app);
    if !app.is_dir() {
        eprintln!("[convertee 번들 검사 실패] 앱 번들이 없습니다: {}", app.display());
        return ExitCode::FAILURE;
    }
    let violations = convertee_guard::check_bundle(&root, app, main_exe);
    if violations.is_empty() {
        println!("[convertee 번들 검사 통과] 별도 실행 파일 없음: {}", app.display());
        return ExitCode::SUCCESS;
    }
    eprintln!("[convertee 번들 검사 실패] 허락받지 않은 것이 번들에 있습니다.");
    for v in &violations {
        eprintln!("  - {v}");
    }
    ExitCode::FAILURE
}
