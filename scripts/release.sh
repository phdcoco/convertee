#!/bin/sh
# 릴리스 빌드를 한 번 하고, 만든 앱 번들에 별도 실행 파일이 없는지 검사한다.
# 소스 검사는 build.rs가 컴파일 전에 한다. 번들 검사가 실패하면 번들을 지우고 멈춘다.
set -eu
cd "$(dirname "$0")/.."
: "${CARGO_TARGET_DIR:=$HOME/.cargo-target}"
export CARGO_TARGET_DIR

(cd src-tauri && cargo tauri build --bundles app)

APP="$CARGO_TARGET_DIR/release/bundle/macos/convertee.app"
if ! cargo run -q -p convertee-guard -- bundle "$APP"; then
    rm -rf "$APP"
    echo "번들 검사에 실패해 번들을 지웠습니다."
    exit 1
fi
du -sh "$APP"
