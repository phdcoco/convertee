# convertee

파일을 창에 끌어다 놓으면 종류를 알아보고, 버튼 한 번으로 다른 형식으로 바꾸는 맥(애플 실리콘) 앱입니다.
인터넷 없이 돌고, 파일은 컴퓨터 밖으로 나가지 않습니다. 지금은 두 가지를 바꿉니다.

- PNG를 JPEG로: 사람이 원본과 차이를 느낄 수 없는 가장 낮은 화질을 고릅니다. 파일 이름 끝(.jpg 또는 .jpeg)은 바꾸기 버튼 옆에서 고릅니다. 둘은 같은 형식입니다.
- PPTX를 PDF로: 슬라이드 한 장이 PDF 한 쪽이 됩니다. 슬라이드를 그림으로 담기 때문에 PDF의 글자는 선택하거나 검색할 수 없습니다. 윈도우 글꼴(맑은 고딕 등)은 맥 글꼴로 대신 그립니다.

## 앱 켜기

    open ~/.cargo-target/release/bundle/macos/convertee.app

응용 프로그램 폴더에 두고 쓰려면 한 번 복사합니다.

    cp -R ~/.cargo-target/release/bundle/macos/convertee.app /Applications/

## 다시 빌드하기

릴리스 빌드 한 번과 앱 번들 검사를 함께 합니다.

    ./scripts/release.sh

고치는 동안에는 이것만 돌립니다.

    cargo check --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

시험에 쓰는 실제 PNG 예시는 처음 한 번 만듭니다 (이 맥의 사진과 아이콘을 복사합니다).

    ./scripts/make-fixtures.sh

PPTX 시험에는 직접 고른 PPTX 한 개가 필요합니다. 경로를 주면 tests/fixtures/lecture.pptx로 복사합니다 (git에는 올라가지 않습니다).

    CONVERTEE_PPTX_SAMPLE=/경로/발표.pptx ./scripts/make-fixtures.sh

변환 한 번에 걸리는 시간과 결과 크기를 잽니다.

    cargo run --release -p convertee-core --example measure -- tests/fixtures/photo.png tests/fixtures/lecture.pptx

## 빌드 폴더 지우기

빌드 폴더는 CARGO_TARGET_DIR(~/.cargo-target) 한 곳입니다. 지워도 다음 빌드 때 다시 만들어집니다.

    rm -rf ~/.cargo-target

## 걸린 시간 기록 보기

환경 변수 CONVERTEE_TIMING을 주고 켜면 켜는 시간과 변환 시간을 기록합니다. 파일 이름과 경로는 기록하지 않습니다.

    open --env CONVERTEE_TIMING=1 --stderr ~/convertee-timing.log ~/.cargo-target/release/bundle/macos/convertee.app

## 지키는 조건

핵심 계산(파일 변환)은 러스트가 합니다. 파이썬, Node 같은 별도 프로그램을 쓰면 빌드가 멈춥니다.
자세한 규칙은 CLAUDE.md에 있습니다.
