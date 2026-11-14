fn main() {
    // 컴파일 전에 0번 조건(핵심 계산을 별도 프로그램에 맡기지 않는다)을 검사한다.
    // 위반이 있으면 panic해서 cargo check, test, build가 모두 멈춘다.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    convertee_guard::enforce(&root);

    tauri_build::build()
}
