//! convertee 창과 명령. 핵심 계산은 convertee-core를 부르기만 한다.
//!
//! 보안: 화면(웹뷰)은 파일 경로를 직접 넘기지 못한다. 사용자가 끌어다 놓은 파일만
//! 러스트가 번호를 매겨 기억하고, 화면은 그 번호로만 부탁할 수 있다.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use convertee_core::{FileKind, HEAD_LEN, Target, detect, png_to_jpeg, pptx_to_pdf};
use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, WindowEvent};

/// 한 파일을 통째로 읽을 때의 상한
const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 측정용 기록. CONVERTEE_TIMING 환경 변수가 있을 때만 표준 오류로 쓴다. 파일 이름이나 경로는 쓰지 않는다.
fn timing(message: &str) {
    if std::env::var_os("CONVERTEE_TIMING").is_some() {
        let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        eprintln!("CONVERTEE_TIMING unix_ms={unix_ms} {message}");
    }
}

struct Entry {
    path: PathBuf,
    kind: FileKind,
    prepared: Option<Prepared>,
}

struct Prepared {
    target: Target,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Files {
    next_id: u64,
    entries: HashMap<u64, Entry>,
}

type AppState = Mutex<Files>;

/// 사용자가 고른 설정. 앱 데이터 폴더의 settings.txt에 "이름=값" 줄로 남긴다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Settings {
    /// JPEG 파일 이름 끝. "jpg" 또는 "jpeg" (같은 형식이고 이름만 다르다)
    jpeg_extension: &'static str,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { jpeg_extension: "jpg" }
    }
}

/// 화면이 고를 수 있는 JPEG 확장자. 이 둘 말고는 받지 않는다.
fn jpeg_extension_from(value: &str) -> Option<&'static str> {
    match value {
        "jpg" => Some("jpg"),
        "jpeg" => Some("jpeg"),
        _ => None,
    }
}

impl Settings {
    fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            if let Some(("jpeg_extension", value)) = line.split_once('=')
                && let Some(ext) = jpeg_extension_from(value.trim())
            {
                settings.jpeg_extension = ext;
            }
        }
        settings
    }

    fn to_text(&self) -> String {
        format!("jpeg_extension={}\n", self.jpeg_extension)
    }

    fn extension(&self, target: Target) -> &'static str {
        match target {
            Target::Jpeg => self.jpeg_extension,
            Target::Pdf => target.extension(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join("settings.txt"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| Settings::parse(&text))
        .unwrap_or_default()
}

#[derive(Serialize)]
struct SettingsInfo {
    jpeg_extension: &'static str,
}

/// 지금 설정을 알려 준다.
#[tauri::command]
fn settings(app: AppHandle) -> Result<SettingsInfo, String> {
    let settings = app.state::<Mutex<Settings>>();
    let settings = settings.lock().map_err(|_| "내부 오류")?;
    Ok(SettingsInfo { jpeg_extension: settings.jpeg_extension })
}

/// JPEG 확장자를 바꾸고 다음에 켤 때도 쓰도록 남긴다.
#[tauri::command]
fn set_jpeg_extension(app: AppHandle, extension: String) -> Result<(), String> {
    let ext = jpeg_extension_from(&extension).ok_or("jpg 또는 jpeg만 고를 수 있습니다")?;
    let state = app.state::<Mutex<Settings>>();
    let mut settings = state.lock().map_err(|_| "내부 오류")?;
    settings.jpeg_extension = ext;
    let path = settings_path(&app).ok_or("설정 폴더를 알 수 없습니다")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("설정을 남기지 못했습니다: {e}"))?;
    }
    std::fs::write(&path, settings.to_text()).map_err(|e| format!("설정을 남기지 못했습니다: {e}"))
}

#[derive(Serialize, Clone)]
struct TargetInfo {
    id: &'static str,
    label: &'static str,
}

#[derive(Serialize, Clone)]
struct DroppedFile {
    id: u64,
    name: String,
    size: u64,
    kind: &'static str,
    targets: Vec<TargetInfo>,
    error: Option<String>,
}

#[derive(Serialize, Default)]
struct PrepareInfo {
    out_size: u64,
    /// JPEG: 고른 화질
    quality: Option<u8>,
    /// JPEG: 원본과 달라 보이는 정도 (butteraugli 거리)
    distance: Option<f64>,
    /// JPEG: 투명한 부분이 있었는지
    had_alpha: bool,
    /// PDF: 쪽 수
    pages: Option<usize>,
}

#[derive(Serialize)]
struct SaveInfo {
    name: String,
    size: u64,
}

fn target_id(t: Target) -> &'static str {
    match t {
        Target::Jpeg => "jpeg",
        Target::Pdf => "pdf",
    }
}

fn target_from_id(id: &str) -> Option<Target> {
    match id {
        "jpeg" => Some(Target::Jpeg),
        "pdf" => Some(Target::Pdf),
        _ => None,
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 끌어다 놓은 경로 하나를 확인해 목록에 넣는다.
fn register(files: &mut Files, path: PathBuf) -> DroppedFile {
    files.next_id += 1;
    let id = files.next_id;
    let name = display_name(&path);
    let fail = |error: String| DroppedFile { id, name: name.clone(), size: 0, kind: "", targets: vec![], error: Some(error) };

    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return fail(format!("읽을 수 없습니다: {e}")),
    };
    if meta.is_dir() {
        return fail("폴더는 아직 바꿀 수 없습니다".into());
    }
    let mut head = Vec::with_capacity(HEAD_LEN);
    if let Err(e) = File::open(&path).and_then(|f| f.take(HEAD_LEN as u64).read_to_end(&mut head)) {
        return fail(format!("읽을 수 없습니다: {e}"));
    }
    let kind = detect(&head);
    files.entries.insert(id, Entry { path, kind, prepared: None });
    DroppedFile {
        id,
        name,
        size: meta.len(),
        kind: kind.label(),
        targets: kind.targets().iter().map(|&t| TargetInfo { id: target_id(t), label: t.label() }).collect(),
        error: None,
    }
}

/// 바꾼 결과를 미리 만들어 크기를 잰다. 저장은 하지 않는다.
#[tauri::command]
async fn prepare(app: AppHandle, id: u64, target: String) -> Result<PrepareInfo, String> {
    let target = target_from_id(&target).ok_or("모르는 형식입니다")?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (path, kind) = {
            let files = state.lock().map_err(|_| "내부 오류")?;
            let entry = files.entries.get(&id).ok_or("목록에 없는 파일입니다")?;
            (entry.path.clone(), entry.kind)
        };
        if !kind.targets().contains(&target) {
            return Err(format!("{}은(는) {}로 바꿀 수 없습니다", kind.label(), target.label()));
        }
        let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if len > MAX_INPUT_BYTES {
            return Err("파일이 너무 큽니다".into());
        }
        let started = Instant::now();
        let input = std::fs::read(&path).map_err(|e| format!("읽을 수 없습니다: {e}"))?;
        let (bytes, info) = match target {
            Target::Jpeg => {
                let out = png_to_jpeg(&input).map_err(|e| e.to_string())?;
                let info = PrepareInfo {
                    out_size: out.bytes.len() as u64,
                    quality: Some(out.quality),
                    distance: out.distance,
                    had_alpha: out.had_alpha,
                    ..PrepareInfo::default()
                };
                (out.bytes, info)
            }
            Target::Pdf => {
                let out = pptx_to_pdf(&input).map_err(|e| e.to_string())?;
                let info = PrepareInfo {
                    out_size: out.bytes.len() as u64,
                    pages: Some(out.pages),
                    ..PrepareInfo::default()
                };
                (out.bytes, info)
            }
        };
        timing(&format!(
            "prepare id={id} target={} in_bytes={} out_bytes={} ms={}",
            target_id(target),
            input.len(),
            bytes.len(),
            started.elapsed().as_millis()
        ));
        let mut files = state.lock().map_err(|_| "내부 오류")?;
        let entry = files.entries.get_mut(&id).ok_or("목록에서 지워졌습니다")?;
        entry.prepared = Some(Prepared { target, bytes });
        Ok(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 미리 만든 결과를 원본과 같은 폴더에 저장한다. 이미 있는 파일은 덮어쓰지 않는다.
#[tauri::command]
async fn save(app: AppHandle, id: u64) -> Result<SaveInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (path, prepared) = {
            let mut files = state.lock().map_err(|_| "내부 오류")?;
            let entry = files.entries.get_mut(&id).ok_or("목록에 없는 파일입니다")?;
            let prepared = entry.prepared.take().ok_or("아직 준비되지 않았습니다")?;
            (entry.path.clone(), prepared)
        };
        let extension = {
            let settings = app.state::<Mutex<Settings>>();
            let settings = settings.lock().map_err(|_| "내부 오류")?;
            settings.extension(prepared.target)
        };
        let started = Instant::now();
        let out_path = write_new_file(&path, extension, &prepared.bytes)?;
        timing(&format!("save id={id} bytes={} ms={}", prepared.bytes.len(), started.elapsed().as_millis()));
        Ok(SaveInfo { name: display_name(&out_path), size: prepared.bytes.len() as u64 })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 목록을 비운다.
#[tauri::command]
fn clear(app: AppHandle) {
    if let Ok(mut files) = app.state::<AppState>().lock() {
        files.entries.clear();
    }
}

/// 원본 옆에 "이름.확장자"로 쓴다. 이미 있으면 "이름 (1).확장자"처럼 번호를 붙인다.
fn write_new_file(source: &Path, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = source.parent().ok_or("저장할 폴더를 알 수 없습니다")?;
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "converted".into());
    for n in 0..10_000u32 {
        let name = if n == 0 { format!("{stem}.{extension}") } else { format!("{stem} ({n}).{extension}") };
        let candidate = dir.join(name);
        // create_new: 이미 있으면 실패하므로 다른 파일을 덮어쓰지 않는다
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(format!("저장하지 못했습니다: {e}"));
                }
                return Ok(candidate);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("저장하지 못했습니다: {e}")),
        }
    }
    Err("같은 이름의 파일이 너무 많습니다".into())
}

fn main() {
    timing("main_started");
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let settings = load_settings(app.handle());
            app.manage(Mutex::new(settings));
            Ok(())
        })
        .on_page_load(|_, payload| {
            if payload.event() == PageLoadEvent::Finished {
                timing("page_loaded");
            }
        })
        .invoke_handler(tauri::generate_handler![prepare, save, clear, settings, set_jpeg_extension])
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                let state = window.state::<AppState>();
                let dropped: Vec<DroppedFile> = match state.lock() {
                    Ok(mut files) => paths.iter().cloned().map(|p| register(&mut files, p)).collect(),
                    Err(_) => return,
                };
                let _ = window.emit("files-dropped", dropped);
            }
        })
        .run(tauri::generate_context!())
        .expect("convertee를 켜지 못했습니다");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("convertee-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 원본_옆에_같은_이름으로_저장한다() {
        let dir = temp_dir("same-name");
        let src = dir.join("사진.png");
        std::fs::write(&src, b"png").unwrap();
        let out = write_new_file(&src, "jpg", b"jpg").unwrap();
        assert_eq!(out, dir.join("사진.jpg"));
        assert_eq!(std::fs::read(&out).unwrap(), b"jpg");
    }

    #[test]
    fn 이미_있으면_덮어쓰지_않고_번호를_붙인다() {
        let dir = temp_dir("numbered");
        let src = dir.join("a.png");
        std::fs::write(&src, b"png").unwrap();
        std::fs::write(dir.join("a.jpg"), b"old").unwrap();
        let out = write_new_file(&src, "jpg", b"new").unwrap();
        assert_eq!(out, dir.join("a (1).jpg"));
        assert_eq!(std::fs::read(dir.join("a.jpg")).unwrap(), b"old");
    }

    #[test]
    fn 확장자가_jpg인_원본도_덮어쓰지_않는다() {
        let dir = temp_dir("misnamed");
        // 내용은 PNG인데 이름이 .jpg인 파일
        let src = dir.join("b.jpg");
        std::fs::write(&src, b"\x89PNG\r\n\x1a\n").unwrap();
        let out = write_new_file(&src, "jpg", b"new").unwrap();
        assert_eq!(out, dir.join("b (1).jpg"));
        assert_eq!(std::fs::read(&src).unwrap(), b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn jpeg_확장자_설정을_읽고_쓴다() {
        assert_eq!(Settings::default().jpeg_extension, "jpg");
        let jpeg = Settings::parse("jpeg_extension=jpeg\n");
        assert_eq!(jpeg.extension(Target::Jpeg), "jpeg");
        assert_eq!(jpeg.extension(Target::Pdf), "pdf");
        assert_eq!(Settings::parse(&jpeg.to_text()), jpeg);
        // 모르는 값이나 깨진 파일은 기본값
        assert_eq!(Settings::parse("jpeg_extension=exe\n"), Settings::default());
        assert_eq!(Settings::parse("아무 글"), Settings::default());
        assert_eq!(jpeg_extension_from("../x"), None);
    }

    #[test]
    fn pptx는_pdf로_바꿀_수_있다() {
        let dir = temp_dir("pptx");
        let src = dir.join("발표.pptx");
        std::fs::write(&src, b"PK\x03\x04....[Content_Types].xml....ppt/presentation.xml").unwrap();
        let mut files = Files::default();
        let d = register(&mut files, src);
        assert_eq!(d.kind, "파워포인트(PPTX)");
        assert_eq!(d.targets.iter().map(|t| t.id).collect::<Vec<_>>(), ["pdf"]);
        assert_eq!(target_from_id("pdf"), Some(Target::Pdf));
    }

    #[test]
    fn 끌어다_놓은_파일은_내용으로_종류를_알아본다() {
        let dir = temp_dir("register");
        let src = dir.join("이름만_jpg.jpg");
        std::fs::write(&src, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        let mut files = Files::default();
        let d = register(&mut files, src);
        assert_eq!(d.kind, "PNG 이미지");
        assert_eq!(d.targets.len(), 1);
        assert!(files.entries.contains_key(&d.id));

        let folder = register(&mut files, dir.clone());
        assert!(folder.error.is_some());
        assert!(!files.entries.contains_key(&folder.id));
    }
}
