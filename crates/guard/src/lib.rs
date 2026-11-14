//! 0번 조건 검사: 핵심 계산을 파이썬, Node 같은 별도 프로그램에 맡기지 않는다.
//!
//! 앱 소스와 앱 번들에서 아래를 찾는다. 허락받은 것(guard/allow.txt)이 아니면 실패한다.
//! - 별도 프로그램을 켜는 코드 (Command, sidecar, 셸 플러그인 등)
//! - 파이썬이나 자바스크립트 엔진을 앱에 넣는 라이브러리
//! - 별도 실행 파일 (실행 권한, Mach-O, 스크립트 첫 줄 #! 등)
//! - 핵심 계산을 화면(웹뷰)에서 하는 코드, 외부 접속 코드
//!
//! 그리고 guard/core-functions.txt에 적힌 핵심 계산 함수가 러스트 코드 안에 있는지 확인한다.
//!
//! 두 build.rs(crates/core, src-tauri)가 enforce를 부르므로 cargo check, test, build가
//! 시작될 때마다 돌고, 실패하면 빌드가 멈춘다.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// 검사하는 앱 소스 폴더 (작업 폴더 기준)
pub const SCAN_DIRS: &[&str] = &["crates", "src-tauri", "ui"];

/// 검사에서 빼는 폴더. guard는 찾을 글자를 담고 있고 앱에 들어가지 않는다. gen은 Tauri가 만든다.
pub const SKIP_DIRS: &[&str] = &["crates/guard", "src-tauri/gen", "src-tauri/target"];

pub const ALLOW_FILE: &str = "guard/allow.txt";
pub const CORE_FUNCTIONS_FILE: &str = "guard/core-functions.txt";

const WHY_LAUNCH: &str = "별도 프로그램을 켜는 코드";
const WHY_ENGINE: &str = "파이썬이나 자바스크립트 엔진을 앱 안에 넣는 것";
const WHY_UI_COMPUTE: &str = "핵심 계산을 화면에서 하는 코드";
const WHY_NETWORK: &str = "외부 접속";
const WHY_EXECUTABLE: &str = "별도 실행 파일";

/// 러스트 소스에서 찾을 글자
const RUST_RULES: &[(&str, &str)] = &[
    ("Command::new", WHY_LAUNCH),
    ("process::Command", WHY_LAUNCH),
    ("os::unix::process", WHY_LAUNCH),
    ("sidecar", WHY_LAUNCH),
    ("tauri_plugin_shell", WHY_LAUNCH),
    ("tauri_plugin_process", WHY_LAUNCH),
    ("posix_spawn", WHY_LAUNCH),
    ("libc::fork", WHY_LAUNCH),
    ("libc::exec", WHY_LAUNCH),
    ("libc::system", WHY_LAUNCH),
    ("pyo3", WHY_ENGINE),
    ("deno_core", WHY_ENGINE),
    ("rquickjs", WHY_ENGINE),
    ("boa_engine", WHY_ENGINE),
];

/// 화면 소스(js, html, css)에서 찾을 글자
const UI_RULES: &[(&str, &str)] = &[
    ("plugin:shell", WHY_LAUNCH),
    ("__TAURI__.shell", WHY_LAUNCH),
    ("Command.create", WHY_LAUNCH),
    ("sidecar", WHY_LAUNCH),
    ("toBlob", WHY_UI_COMPUTE),
    ("toDataURL", WHY_UI_COMPUTE),
    ("getImageData", WHY_UI_COMPUTE),
    ("OffscreenCanvas", WHY_UI_COMPUTE),
    ("createImageBitmap", WHY_UI_COMPUTE),
    ("WebAssembly", WHY_UI_COMPUTE),
    ("fetch(", WHY_NETWORK),
    ("XMLHttpRequest", WHY_NETWORK),
    ("WebSocket", WHY_NETWORK),
    ("EventSource", WHY_NETWORK),
    ("http://", WHY_NETWORK),
    ("https://", WHY_NETWORK),
];

/// Tauri 설정(json)에서 찾을 글자
const CONFIG_RULES: &[(&str, &str)] = &[
    ("externalBin", WHY_LAUNCH),
    ("sidecar", WHY_LAUNCH),
    ("\"shell", WHY_LAUNCH),
    ("shell:", WHY_LAUNCH),
    ("process:", WHY_LAUNCH),
];

/// Cargo.toml 의존성 이름과 Cargo.lock 패키지 이름
const CRATE_RULES: &[(&str, &str)] = &[
    ("tauri-plugin-shell", WHY_LAUNCH),
    ("tauri-plugin-process", WHY_LAUNCH),
    ("duct", WHY_LAUNCH),
    ("subprocess", WHY_LAUNCH),
    ("pyo3", WHY_ENGINE),
    ("cpython", WHY_ENGINE),
    ("rustpython", WHY_ENGINE),
    ("rustpython-vm", WHY_ENGINE),
    ("deno_core", WHY_ENGINE),
    ("v8", WHY_ENGINE),
    ("rquickjs", WHY_ENGINE),
    ("boa_engine", WHY_ENGINE),
    ("mlua", WHY_ENGINE),
];

/// 우리 Cargo.toml에 직접 넣으면 안 되는 네트워크 의존성.
/// Cargo.lock은 보지 않는다. Tauri가 다른 운영체제용으로 reqwest를 적어 두기 때문이다.
const DIRECT_NETWORK_CRATES: &[&str] = &["reqwest", "ureq", "hyper", "isahc", "surf", "attohttpc", "tauri-plugin-http"];

/// 실행 파일이나 스크립트로 볼 확장자
const EXECUTABLE_EXTENSIONS: &[&str] = &["py", "pyc", "sh", "command", "exe", "dll", "dylib", "so", "node", "jar", "bat"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// 작업 폴더 기준 경로 (번들은 "bundle/..." 로 시작)
    pub file: String,
    /// 찾은 것
    pub found: String,
    pub why: String,
}

impl Violation {
    fn new(file: &str, found: &str, why: &str) -> Self {
        Violation { file: file.to_string(), found: found.to_string(), why: why.to_string() }
    }

    /// guard/allow.txt에 적을 때 쓰는 이름
    pub fn key(&self) -> String {
        format!("{}: {}", self.file, self.found)
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.key(), self.why)
    }
}

/// 작업 폴더 전체를 검사하고, 허락받지 않은 것만 돌려준다.
pub fn check_workspace(root: &Path) -> Vec<Violation> {
    let mut found = Vec::new();
    for dir in SCAN_DIRS {
        for path in walk(&root.join(dir), root) {
            check_file(root, &path, &mut found);
        }
    }
    check_lock(root, &mut found);
    check_core_functions(root, &mut found);
    filter_allowed(root, found)
}

/// 만든 앱 번들(.app)을 검사한다. Contents/MacOS에는 앱 실행 파일 하나만 있어야 한다.
pub fn check_bundle(root: &Path, app: &Path, main_exe: &str) -> Vec<Violation> {
    let mut found = Vec::new();
    let macos = app.join("Contents/MacOS");
    match fs::read_dir(&macos) {
        Ok(entries) => {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name != main_exe {
                    found.push(Violation::new(&format!("bundle/Contents/MacOS/{name}"), "실행 파일", WHY_EXECUTABLE));
                }
            }
        }
        Err(_) => found.push(Violation::new("bundle/Contents/MacOS", "폴더 없음", "앱 번들이 아님")),
    }
    let main_path = macos.join(main_exe);
    for path in walk(app, app) {
        if path == main_path {
            continue;
        }
        let rel = format!("bundle/{}", rel(app, &path));
        if let Some(why) = executable_kind(&path) {
            found.push(Violation::new(&rel, why, WHY_EXECUTABLE));
        }
        let lower = rel.to_lowercase();
        for name in ["python", "node"] {
            if file_name(&path).to_lowercase().starts_with(name) {
                found.push(Violation::new(&lower, name, WHY_EXECUTABLE));
            }
        }
    }
    filter_allowed(root, found)
}

/// build.rs에서 부른다. 위반이 있으면 panic해서 빌드를 멈춘다.
pub fn enforce(root: &Path) {
    for dir in SCAN_DIRS {
        println!("cargo:rerun-if-changed={}", root.join(dir).display());
    }
    println!("cargo:rerun-if-changed={}", root.join("guard").display());
    println!("cargo:rerun-if-changed={}", root.join("Cargo.lock").display());

    let violations = check_workspace(root);
    if !violations.is_empty() {
        let list: Vec<String> = violations.iter().map(|v| format!("  - {v}")).collect();
        panic!(
            "\n\n[convertee 검사 실패] 허락받지 않은 것이 있어 빌드를 멈춥니다.\n{}\n\n\
             핵심 계산은 러스트가 해야 합니다. 별도 프로그램이 꼭 필요하면 사용자에게 이유와 대안을 말하고\n\
             허락을 받은 뒤, 위 줄의 \"파일: 찾은 것\" 부분을 {ALLOW_FILE}에 적으세요.\n\n",
            list.join("\n")
        );
    }
}

fn check_file(root: &Path, path: &Path, found: &mut Vec<Violation>) {
    let rel_path = rel(root, path);
    if let Some(why) = executable_kind(path) {
        found.push(Violation::new(&rel_path, why, WHY_EXECUTABLE));
    }
    if rel_path.split('/').any(|part| part == "binaries") {
        found.push(Violation::new(&rel_path, "binaries 폴더", WHY_EXECUTABLE));
    }
    let Ok(text) = fs::read_to_string(path) else {
        return; // 글자가 아닌 파일(아이콘 등)은 위의 실행 파일 검사만 한다
    };
    let rules: &[(&str, &str)] = match extension(path).as_str() {
        "rs" => RUST_RULES,
        "js" | "mjs" | "ts" | "html" | "css" => UI_RULES,
        "json" => CONFIG_RULES,
        _ => &[],
    };
    for (needle, why) in rules {
        if text.contains(needle) {
            found.push(Violation::new(&rel_path, needle, why));
        }
    }
    if file_name(path) == "Cargo.toml" {
        for dep in dependency_names(&text) {
            if let Some((_, why)) = CRATE_RULES.iter().find(|(n, _)| *n == dep) {
                found.push(Violation::new(&rel_path, &dep, why));
            }
            if DIRECT_NETWORK_CRATES.contains(&dep.as_str()) {
                found.push(Violation::new(&rel_path, &dep, WHY_NETWORK));
            }
        }
    }
}

/// Cargo.lock에 별도 프로그램을 켜거나 엔진을 넣는 crate가 끼어 있는지 본다 (간접 의존성 포함).
fn check_lock(root: &Path, found: &mut Vec<Violation>) {
    let Ok(text) = fs::read_to_string(root.join("Cargo.lock")) else {
        return;
    };
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("name = \"").and_then(|s| s.strip_suffix('"'))
            && let Some((_, why)) = CRATE_RULES.iter().find(|(n, _)| *n == name)
        {
            found.push(Violation::new("Cargo.lock", name, why));
        }
    }
}

/// guard/core-functions.txt의 "파일 | 찾을 글" 줄마다, 그 파일이 러스트 소스이고 글이 들어 있는지 본다.
fn check_core_functions(root: &Path, found: &mut Vec<Violation>) {
    let Ok(text) = fs::read_to_string(root.join(CORE_FUNCTIONS_FILE)) else {
        found.push(Violation::new(CORE_FUNCTIONS_FILE, "파일 없음", "핵심 계산 함수 목록이 없음"));
        return;
    };
    let mut count = 0;
    for line in meaningful_lines(&text) {
        let Some((file, needle)) = line.split_once('|') else {
            found.push(Violation::new(CORE_FUNCTIONS_FILE, line, "\"파일 | 찾을 글\" 모양이 아님"));
            continue;
        };
        let (file, needle) = (file.trim(), needle.trim());
        count += 1;
        if !file.ends_with(".rs") {
            found.push(Violation::new(file, needle, "핵심 계산이 러스트 파일에 있지 않음"));
            continue;
        }
        match fs::read_to_string(root.join(file)) {
            Ok(src) if src.contains(needle) => {}
            Ok(_) => found.push(Violation::new(file, needle, "핵심 계산 함수를 러스트 코드에서 찾지 못함")),
            Err(_) => found.push(Violation::new(file, needle, "핵심 계산 파일이 없음")),
        }
    }
    if count == 0 {
        found.push(Violation::new(CORE_FUNCTIONS_FILE, "비어 있음", "핵심 계산 함수 목록이 비어 있음"));
    }
}

fn filter_allowed(root: &Path, found: Vec<Violation>) -> Vec<Violation> {
    let allowed: Vec<String> = fs::read_to_string(root.join(ALLOW_FILE))
        .map(|t| meaningful_lines(&t).map(str::to_string).collect())
        .unwrap_or_default();
    let mut out: Vec<Violation> = found.into_iter().filter(|v| !allowed.contains(&v.key())).collect();
    out.sort_by_key(Violation::key);
    out.dedup();
    out
}

/// 실행 파일로 보이면 그 이유를 돌려준다.
fn executable_kind(path: &Path) -> Option<&'static str> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path)
            && meta.permissions().mode() & 0o111 != 0
        {
            return Some("실행 권한");
        }
    }
    if EXECUTABLE_EXTENSIONS.contains(&extension(path).as_str()) {
        return Some("실행 파일 확장자");
    }
    let mut head = [0u8; 4];
    let n = fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let head = &head[..n];
    let machos: [[u8; 4]; 5] = [
        [0xfe, 0xed, 0xfa, 0xce],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xce, 0xfa, 0xed, 0xfe],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xca, 0xfe, 0xba, 0xbe],
    ];
    if machos.iter().any(|m| head == m) {
        return Some("Mach-O 실행 파일");
    }
    if head.starts_with(b"\x7fELF") {
        return Some("ELF 실행 파일");
    }
    if head.starts_with(b"MZ") {
        return Some("윈도우 실행 파일");
    }
    if head.starts_with(b"#!") {
        return Some("스크립트 (#!)");
    }
    None
}

/// Cargo.toml에서 의존성 이름을 뽑는다. "이름 = ..." 줄과 [dependencies.이름] 머리를 본다.
fn dependency_names(toml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let header = line.trim_matches(|c| c == '[' || c == ']');
            in_deps = header.ends_with("dependencies");
            if let Some((table, name)) = header.rsplit_once('.')
                && table.ends_with("dependencies")
            {
                names.push(name.trim_matches('"').to_string());
            }
            continue;
        }
        if in_deps && let Some((key, _)) = line.split_once('=') {
            let key = key.trim().trim_matches('"');
            let key = key.split('.').next().unwrap_or(key);
            if !key.is_empty() && !key.starts_with('#') {
                names.push(key.to_string());
            }
        }
    }
    names
}

fn meaningful_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// 폴더 안의 모든 파일. SKIP_DIRS는 뺀다.
fn walk(dir: &Path, root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if SKIP_DIRS.contains(&rel(root, &p).as_str()) {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(_) => out.push(p),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
    }

    /// 깨끗한 작은 작업 폴더를 만든다. 핵심 계산 함수 목록과 그 함수만 들어 있다.
    fn clean_workspace(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("convertee-guard-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/core/src")).unwrap();
        fs::create_dir_all(root.join("src-tauri/src")).unwrap();
        fs::create_dir_all(root.join("ui")).unwrap();
        fs::create_dir_all(root.join("guard")).unwrap();
        fs::write(root.join("crates/core/src/lib.rs"), "pub fn png_to_jpeg(png: &[u8]) {}\n").unwrap();
        fs::write(root.join("src-tauri/src/main.rs"), "fn main() { core::png_to_jpeg(&[]); }\n").unwrap();
        fs::write(root.join("ui/app.js"), "invoke('prepare');\n").unwrap();
        fs::write(root.join(CORE_FUNCTIONS_FILE), "crates/core/src/lib.rs | pub fn png_to_jpeg(\n").unwrap();
        root
    }

    fn keys(v: &[Violation]) -> Vec<String> {
        v.iter().map(Violation::key).collect()
    }

    #[test]
    fn 실제_프로젝트는_검사를_통과한다() {
        let v = check_workspace(&workspace_root());
        assert!(v.is_empty(), "위반: {v:?}");
    }

    #[test]
    fn 깨끗한_작업_폴더는_통과한다() {
        assert!(check_workspace(&clean_workspace("clean")).is_empty());
    }

    #[test]
    fn 파이썬을_켜는_러스트_코드를_막는다() {
        let root = clean_workspace("python");
        fs::write(
            root.join("src-tauri/src/convert.rs"),
            "fn f() { std::process::Command::new(\"python3\").arg(\"convert.py\").status(); }\n",
        )
        .unwrap();
        let v = keys(&check_workspace(&root));
        assert!(v.contains(&"src-tauri/src/convert.rs: Command::new".to_string()), "{v:?}");
        assert!(v.contains(&"src-tauri/src/convert.rs: process::Command".to_string()), "{v:?}");
    }

    #[test]
    fn sidecar_설정과_셸_플러그인을_막는다() {
        let root = clean_workspace("sidecar");
        fs::write(root.join("src-tauri/tauri.conf.json"), r#"{"bundle":{"externalBin":["binaries/node"]}}"#).unwrap();
        fs::write(
            root.join("src-tauri/Cargo.toml"),
            "[dependencies]\ntauri = \"2\"\ntauri-plugin-shell = \"2\"\n\n[dependencies.pyo3]\nversion = \"0.22\"\n",
        )
        .unwrap();
        let v = keys(&check_workspace(&root));
        assert!(v.contains(&"src-tauri/tauri.conf.json: externalBin".to_string()), "{v:?}");
        assert!(v.contains(&"src-tauri/Cargo.toml: tauri-plugin-shell".to_string()), "{v:?}");
        assert!(v.contains(&"src-tauri/Cargo.toml: pyo3".to_string()), "{v:?}");
    }

    #[test]
    fn 별도_실행_파일을_막는다() {
        let root = clean_workspace("exe");
        fs::create_dir_all(root.join("src-tauri/binaries")).unwrap();
        fs::write(root.join("src-tauri/binaries/node-aarch64-apple-darwin"), [0xcf, 0xfa, 0xed, 0xfe, 0, 0]).unwrap();
        fs::write(root.join("crates/core/convert.py"), "print(1)\n").unwrap();
        let v = keys(&check_workspace(&root));
        assert!(v.contains(&"src-tauri/binaries/node-aarch64-apple-darwin: Mach-O 실행 파일".to_string()), "{v:?}");
        assert!(v.contains(&"crates/core/convert.py: 실행 파일 확장자".to_string()), "{v:?}");
    }

    #[test]
    fn 화면에서_변환하거나_외부로_접속하는_코드를_막는다() {
        let root = clean_workspace("ui");
        fs::write(root.join("ui/app.js"), "canvas.toBlob(cb, 'image/jpeg'); fetch('https://x.example');\n").unwrap();
        let v = keys(&check_workspace(&root));
        assert!(v.contains(&"ui/app.js: toBlob".to_string()), "{v:?}");
        assert!(v.contains(&"ui/app.js: fetch(".to_string()), "{v:?}");
    }

    #[test]
    fn 핵심_계산_함수가_러스트에_없으면_막는다() {
        let root = clean_workspace("missing");
        fs::write(root.join("crates/core/src/lib.rs"), "// 함수를 지웠다\n").unwrap();
        let v = keys(&check_workspace(&root));
        assert!(v.contains(&"crates/core/src/lib.rs: pub fn png_to_jpeg(".to_string()), "{v:?}");

        // 핵심 계산을 자바스크립트 파일로 옮긴 경우
        fs::write(root.join(CORE_FUNCTIONS_FILE), "ui/convert.js | function pngToJpeg(\n").unwrap();
        let v = check_workspace(&root);
        assert!(v.iter().any(|x| x.why == "핵심 계산이 러스트 파일에 있지 않음"), "{v:?}");
    }

    #[test]
    fn 허락받은_것은_통과시킨다() {
        let root = clean_workspace("allowed");
        fs::write(root.join("src-tauri/src/x.rs"), "fn f() { std::process::Command::new(\"soffice\"); }\n").unwrap();
        fs::write(
            root.join(ALLOW_FILE),
            "# 사용자 허락: 2026-11-14\nsrc-tauri/src/x.rs: Command::new\nsrc-tauri/src/x.rs: process::Command\n",
        )
        .unwrap();
        assert!(check_workspace(&root).is_empty());
    }

    #[test]
    fn 번들에_다른_실행_파일이_있으면_막는다() {
        let root = clean_workspace("bundle");
        let app = root.join("convertee.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        fs::write(app.join("Contents/MacOS/convertee"), [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        fs::write(app.join("Contents/Resources/icon.icns"), b"icns").unwrap();
        assert!(check_bundle(&root, &app, "convertee").is_empty());

        fs::write(app.join("Contents/MacOS/python3"), [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        fs::write(app.join("Contents/Resources/run.sh"), "#!/bin/sh\n").unwrap();
        let v = keys(&check_bundle(&root, &app, "convertee"));
        assert!(v.contains(&"bundle/Contents/MacOS/python3: 실행 파일".to_string()), "{v:?}");
        assert!(v.contains(&"bundle/Contents/Resources/run.sh: 실행 파일 확장자".to_string()), "{v:?}");
    }

    #[test]
    fn 의존성_이름을_뽑는다() {
        let toml = "[package]\nname = \"x\"\n[dependencies]\nimage = { version = \"0.25\" }\nserde.workspace = true\n[build-dependencies.tauri-build]\nversion = \"2\"\n";
        assert_eq!(dependency_names(toml), vec!["image", "serde", "tauri-build"]);
    }
}
