# convertee 작업 규칙

파일을 끌어다 놓으면 종류를 알아보고, 버튼 한 번으로 다른 형식으로 바꾸는 맥(애플 실리콘) 데스크톱 앱이다.
이 파일과 AGENTS.md는 같은 내용을 담는다. 한쪽을 고치면 다른 쪽도 고친다.

## 앱이 지킬 조건

- 화면은 Tauri v2로 만들고 앱의 계산은 러스트가 한다. 일렉트론은 쓰지 않는다.
- 핵심 계산(파일 형식 변환)을 파이썬, Node 같은 별도 프로그램에 맡기지 않는다.
- 별도 프로그램이 꼭 필요하면 만들기 전에 이유와 대안을 사용자에게 말하고 허락을 받는다. 허락받은 것만 guard/allow.txt에 적는다.
- 인터넷 없이 돈다. 파일은 컴퓨터 밖으로 나가지 않는다. 네트워크 crate를 넣지 않고, 화면의 CSP로 외부 접속을 막는다.
- 사용자가 정하지 않은 것을 혼자 정하지 않는다. 모르면 묻는다.
- 숫자는 추정하지 않고 실제로 재서 보고한다.

## 사용자가 정한 동작

- 바꾼 파일은 원본과 같은 폴더에 같은 이름으로 저장한다. 같은 이름의 파일이 이미 있으면 덮어쓰지 않고 "이름 (1).jpg"처럼 번호를 붙인다. 저장 위치는 나중에 사용자가 바꿀 수 있게 될 수 있다.
- JPEG 화질은 화질 먼저로 정한다. 사람이 원본과 차이를 느낄 수 없는 가장 낮은 화질(butteraugli 거리 1.0 이하)을 고른다. 크기는 원본보다 커져도 되지만, 바꾸기 전에 예상 크기를 보여 준다.
- PNG의 투명한 부분은 검은색으로 채운다.
- JPEG 파일 이름 끝(.jpg 또는 .jpeg)은 앱을 쓰는 사람이 바꾸기 버튼 옆에서 고른다. 기본은 .jpg이고, 고른 값은 ~/Library/Application Support/com.phdcoco.convertee/settings.txt에 남는다.
- PPTX를 PDF로 바꾸는 방식은 사용자가 맡겼다. 지금은 슬라이드마다 배율 3(1인치 288픽셀)으로 그린 PNG를 PDF 한 쪽에 담는다. 그래서 PDF의 글자는 선택하거나 검색할 수 없다. 화면에도 이 한계를 알린다.
- 윈도우 글꼴은 맥 글꼴로 대신 그린다. 맑은 고딕 자리에는 Apple SD 산돌고딕 Neo를 쓰되, 글자 폭을 맑은 고딕의 실제 폭(한글 1.000 em, 공백 0.352 em)으로 고쳐서 쓴다 (메모리 안에서만).
- 첫판은 PNG를 JPEG로. 둘째 판은 PPTX를 PDF로.

## 빌드 환경

- 빌드 폴더는 환경 변수 CARGO_TARGET_DIR로 /Users/ryu/.cargo-target 한 곳에 고정한다 (~/.zshrc에 적혀 있다). 작업 폴더가 여러 개여도 같은 빌드 폴더를 쓴다.
- 러스트 도구(~/.rustup, ~/.cargo)와 빌드 폴더는 내장 디스크에 둔다. 빼서 쓰는 외장 디스크로 옮기지 않는다. 옮겨야 하면 먼저 묻는다.
- 디스크 남은 용량이 20GB보다 적으면 멈추고 알린다.
- 빌드 설정(features, 프로필, RUSTFLAGS)은 아래로 고정한다. 바꾸면 같은 부품을 다시 컴파일하므로, 바꿔야 하면 먼저 묻는다.
  - 프로필: 루트 Cargo.toml의 profile.dev, profile.dev.package."*", profile.release
  - 개발용 빌드: 내 코드는 디버그 정보를 줄 번호만, 가져온 라이브러리는 디버그 정보 없이 최적화 2단계
  - 릴리스 빌드: 최적화 3단계, LTO, codegen-units 1, strip
  - RUSTFLAGS: 비워 둔다
  - features: image는 png와 jpeg만(기본 기능 끔), jpeg-encoder와 butteraugli는 기본값, tauri는 기본값
  - features: betteroffice-pptx는 raster, krilla와 betteroffice-metafile은 기본값, zip은 기본 기능 끄고 deflate-flate2-zlib-rs만 (betteroffice가 이미 쓰는 판과 기능에 맞춘다)
- 빌드는 한 번에 하나만 돌린다. cargo 명령을 동시에 두 개 이상 켜지 않는다.
- 고치는 동안에는 cargo check, cargo clippy, cargo test만 돌린다. 릴리스 빌드는 기능이 다 된 뒤 한 번 한다.
- 단계를 끝낼 때마다 "하는 일 / 누가 하는가 / 파일" 표와 빌드 폴더 크기, 디스크 남은 용량을 보고한다. 핵심 계산 줄에 "러스트"가 아닌 값이 있으면 멈추고 알린다.

## 폴더

- crates/core: 핵심 계산 (파일 종류 알아내기, PNG를 JPEG로, PPTX를 PDF로). Tauri에 의존하지 않는다.
- src-tauri: 창과 명령. 핵심 계산은 crates/core를 부르기만 한다.
- ui: 화면 (HTML 한 장, 빌드 도구와 Node 없음).
- crates/guard: 0번 조건 검사 코드. crates/core와 src-tauri의 build.rs가 부르므로 cargo check, test, build 전에 자동으로 돌고, 위반이 있으면 빌드가 멈춘다.
- guard/allow.txt: 사용자에게 허락받은 별도 프로그램 목록 (지금은 비어 있다).
- guard/core-functions.txt: 핵심 계산 함수와 그 함수가 있어야 할 러스트 파일 목록. 핵심 계산 함수를 새로 만들거나 옮기면 여기도 고친다.
- scripts/release.sh: 릴리스 빌드 한 번과 앱 번들 검사. 번들 검사에 실패하면 번들을 지우고 멈춘다.
- crates/core/examples/measure.rs: 핵심 계산 한 번의 시간을 잰다 (PNG와 PPTX).
- scripts/make-fixtures.sh: 시험용 실제 PNG를 이 맥의 사진과 아이콘으로 만든다. CONVERTEE_PPTX_SAMPLE에 PPTX 경로를 주면 그 파일도 lecture.pptx로 복사한다 (git에 올리지 않는다).
