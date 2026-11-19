# 12. Tab 메타데이터 floating layer

2026-11-19 · v0.5.4. GTK/Win32에 같은 토글 정책을 적용한다. Windows 제품 readiness/보안·실사용 검증은 별도이며 계속 UNTESTED다.

## 동작

- 기본값은 **꺼짐**. main viewer의 Tab/Shift+Tab은 UI 컨트롤 포커스 순회 대신 레이어를 토글한다. 상단 입력을 클릭해도 포커스를 옮기지 않는다.
- `Tab`/`Shift+Tab` 또는 오른쪽 위 `ⓘ` 버튼으로 토글한다. 켜져 있고 현재 이미지에 메타데이터가 있을 때만 버튼 아래 오른쪽에 표시한다. 크기 조절 중에도 오른쪽에 배치하며, 데이터가 없는 페이지·재열기·오류에서는 이전 내용을 숨긴다.
- zoom/pan·전체 화면·toolbar 숨김과 독립적이며 레이어 내부에서 긴 내용을 스크롤한다. 이미지 크기/렌더 viewport는 변경하지 않는다.
- 토글 상태는 프로세스 내 UI 상태이며 저장하지 않는다. 새 실행의 기본값은 off다. 별도 OS 파일/주소 modal 대화상자의 표준 Tab 입력은 유지한다.

## 표시 범위

| 입력 | 표시 |
|---|---|
| PNG/APNG | tEXt·zTXt·iTXt(생성 정보/parameters 등), eXIf |
| JPEG | EXIF, 표준 APP1 XMP, COM 코멘트 |
| WebP | EXIF, XMP |
| GIF/AGIF | Comment extension |
| ZIP/CBZ | 압축 내부 이미지의 같은 metadata |

EXIF는 `kamadak-exif`의 필드/값 표시를 사용한다. XMP는 XML UI가 아니라 plain text다. PNG Latin-1/UTF-8을 구분하며 UTF-8 코멘트의 잘못된 bytes는 대체 문자로 표시할 수 있다.

binary ICC, 임의 vendor 데이터, JPEG extended-XMP 재조립, OS 파일 속성을 모두 표시하는 기능은 아니다. 원본/archive를 수정하거나 별도 metadata 파일·로그·외부 서버에 저장하지 않는다. EXIF에 위치 등 민감한 내용이 포함될 수 있어 기본 off를 유지한다.

## 처리 경계와 상한

1. worker의 기존 bounded input/ZIP spool에서 optional metadata를 추출한 뒤 입력을 되감아 기존 codec으로 디코딩한다. GUI/backend는 비신뢰 이미지/EXIF/XML 파서를 실행하지 않는다.
2. 실제 metadata 읽기 최대 256KiB, payload와 개별 압축 해제 최대 64KiB. 구조 탐색/skip은 유한하며 기존 source byte/seek·worker timeout/메모리 제한을 유지한다. 잘못되거나 큰 optional metadata는 건너뛰거나 중단한다.
3. DTO 최대 32항목·이름 128bytes·값 4096bytes·항목 overhead 포함 총 32KiB. 긴 값은 UTF-8 경계에서 잘라 `…`로 표시한다. 제어/bidi 문자를 제거하고 줄바꿈/탭은 텍스트로 유지한다.
4. wire **v5**의 `FrameInfo.metadata`를 parent가 다시 검증한다. GUI/worker는 같은 빌드여야 한다. source revision/generation·animation next/seek·static cache·frame lease에 연결한다.
5. cache 비용과 GUI/UTF-16/control/일시 문자열 복사의 보수적 8배 예약을 기존 예산에 포함한다. markup 실행·GUI 자동 이미지 로더·worker 보호 우회는 없다.

## 실제 검증

- Linux Rust 97개 통과, 장기 1개 ignored. PNG 일반/압축/국제 텍스트·잘못된/큰 입력·압축 폭탄 상한·JPEG EXIF/COM·WebP XMP·GIF Comment·ZIP·원본 불변·APNG worker next/seek/일반 페이지 전환을 포함한다.
- 실제 GTK `metadata-smoke.py` 3개: metadata 없는 PNG/있는 PNG/CBZ. 기본 off·Tab/Shift+Tab key-signal 토글·포커스 유지·plain text·clear 후 숨김·원본 hash 불변을 검사했다.
- 기존 GTK animation/control 8개·Python 배포 gate 7개·fmt/Clippy/release build 통과.
- Windows-target unit 36개 통과, native symlink 권한 필요 1개 ignored. cross Clippy/PE system-DLL import build와 Wine platform/빈 창/resize/합성 metadata overlay/worker 거부 5개 통과.
- `--smoke-metadata-window`는 신뢰 합성 텍스트의 UI 전용 검사이며 protected 이미지 디코딩 성공이 아니다.
- 로컬 전체 desktop 회귀는 `xvfb-run` 부재로 실행하지 못했다. Linux/패키지 CI에는 desktop 및 새 metadata 검사를 정의하며 실제 결과는 Actions 기록을 따른다.
- key-signal 회귀는 OS 키 입력 주입·실물 EXIF corpus·IME/접근성 인증이 아니다. native Windows DACL 오류·실사용 검증 미완료는 유지한다.

## 재현

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
GSK_RENDERER=cairo python3 tools/metadata-smoke.py --image target/release/ncat-linux
GSK_RENDERER=cairo python3 tools/animation-smoke.py --image target/release/ncat-linux
```

도구는 자체 합성 fixture만 생성하고 경로를 출력한다. 사용자 사진/EXIF/네트워크 계정을 저장소에 복사하지 않는다. CI는 AppImage의 GUI와 worker도 같은 도구로 검사한다.
