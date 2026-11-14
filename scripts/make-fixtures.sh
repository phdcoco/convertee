#!/bin/sh
# 시험용 실제 PNG 예시를 이 맥에 들어 있는 사진과 아이콘으로 만든다.
# 애플의 그림이라 git에 올리지 않는다 (tests/fixtures는 .gitignore에 있다).
set -eu
cd "$(dirname "$0")/.."
OUT=tests/fixtures
mkdir -p "$OUT"
PHOTOS="/System/Library/Templates/Data/Library/Application Support/Apple/Photos/Print Products/Previews"
# 사진 예시: 사진 앱 인쇄 미리보기 (648x509)
cp "$PHOTOS/Portfolio_Preview.pppreview/Contents/Resources/PortfolioLandscape_9DF2FDFA-3A7C-4122-B9D8-1DADA77BBFA1.png" "$OUT/photo.png"
# 투명 예시: 독의 휴지통 아이콘 (256x256, 투명 바탕)
cp "/System/Library/CoreServices/Dock.app/Contents/Resources/s-trashfull@2x.png" "$OUT/transparent.png"
# 큰 사진 예시: 바탕화면 사진을 PNG로 (실제 사용 시간을 잴 때 쓴다)
sips -s format png "/System/Library/Desktop Pictures/Sonoma.heic" --out "$OUT/large.png" >/dev/null
ls -l "$OUT"
