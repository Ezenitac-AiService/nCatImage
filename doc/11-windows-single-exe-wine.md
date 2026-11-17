# 11. 단일 Windows EXE·Wine 개발 계획

## v0.5.3 수정: Windows resize/paint

`WM_SIZE`의 layout/invalidating만으로는 새 디코딩 크기를 반영하지 못했고, `WM_EXITSIZEMOVE`의 reload는 최대화/복원·전체 화면을 처리하지 못했다. 특히 `SetWindowPos`가 `App` mutable borrow 중 동기 재진입하면 기존 callback이 default 처리로 빠져 갱신을 놓쳤다.

수정: 크기 메시지는 borrow와 무관하게 invalidation과 전용 deferred 메시지를 발생시킨다. UI 전체 배경과 보관 중 프레임을 다시 그리며, 새 viewport가 기존 요청과 다르면 100ms debounce 후 backend load를 요청한다. 최소화·종료/동일 view는 재요청하지 않으며, 초기 catalog 완료 뒤에도 viewport를 다시 확인한다. fullscreen/UI 전환은 명시적으로 같은 경로를 깨운다. 상시 resize polling은 없다.

`--smoke-resize`는 빈 실제 창으로 resize/maximize/restore/fullscreen/UI toggle/minimize/restore를 순서대로 호출하고, deferred viewport가 최종 client rect와 일치하는지 및 paint 횟수가 증가하는지 검사한다. Wine에서 통과했으며 **이미지 픽셀/worker/native Windows 실사용 검증과는 별도**다. 사용자 승인으로 v0.5.3 릴리스 빌드에 포함하며, 기존 v0.5.2는 변경하지 않는다. native DACL 오류는 이번 수정 범위가 아니며 UNTESTED/readiness false를 유지한다.

2026-11-17 · 개발 시작. Linux GTK/AppImage는 유지한다. 이 문서와 시험용 Windows EXE 추가를 Windows 제품 완성/공식 지원으로 표현하지 않는다.

**v0.5.2 공개 승인:** 사용자 요청으로 Windows 단일 EXE를 Linux와 같은 프리릴리스에 **UNTESTED 실험판**으로 포함한다. 실제 Windows 환경에서는 아직 테스트하지 않았다. 보호 우회·readiness 승격 없이 별도 opt-in release build를 사용하며 [배포 문서](10-distribution.md)를 따른다. 아래 “공개하지 않음/커밋하지 않음”은 승인 전 단계의 기록이다.

**후속 native CI:** [43971418584](https://github.com/NyarCat/nCatImage/actions/runs/43971418584)에서 최초 Windows 자동 실행을 시도했으나 media 2개가 `private owner/DACL required`로 실패했다. GUI/보호 pipeline 검사는 도달하지 못했다. 따라서 정상 동작 검증은 여전히 미완료이며 UNTESTED/알려진 권한 오류로 공개한다. “아직 테스트하지 않음”은 이 최초 CI 시도 전 기록과 실제 실사용 검증 미완료를 구분해서 읽는다.

## 1. 최종 계약

- 배포 파일과 실행 코드 파일은 `nCatImage.exe` 하나다. GTK/third-party DLL·추가 worker EXE를 동봉하거나 임시 폴더에 풀지 않는다.
- 같은 EXE의 일반 모드는 native Windows GUI, `--worker`는 비신뢰 codec 프로세스다. 프로세스는 분리하며 GUI 안에서 codec을 직접 실행하는 fallback은 금지한다.
- Windows 시스템 DLL은 사용한다. “시스템 API 의존성 없음”이나 Linux에서 직접 실행되는 PE를 의미하지 않는다.
- 초기 pixel IPC는 기존 private 임시 파일/부모 소유 FD 계약을 유지할 수 있다. **임시 데이터 파일까지 0개**는 별도 공유 메모리 단계다. 단일 EXE와 혼동하지 않는다.
- 설정·로그·Wine prefix는 실행 코드가 아니다. 원본은 읽기 전용이며 앱/worker 생성·종료·오류에 원본 bytes가 바뀌지 않아야 한다.

## 2. GUI 결정

Windows에는 GTK 대신 작은 Win32/Rust 셸을 추가한다. 우선 시스템 컨트롤·GDI 표시 경로를 검증하고 GPU renderer/웹 runtime을 추가하지 않는다. 파일/폴더/CBZ, RGBA texture 소유권, 읽기 키, 애니메이션/프레임 UI는 공통 presentation/backend로 연결한다.

Win32 FFI는 Windows crate의 좁은 module에서만 허용하며 handle·pointer·thread·callback 수명을 기록한다. core/presentation/protocol/worker의 safe Rust 원칙은 유지한다. Linux GUI를 Windows 셸로 교체하지 않는다.

## 3. 단계별 작업

| 단계 | 구현 | 종료 조건 |
|---|---|---|
| W0 기반 | Windows lossless 경로, worker 명령/동일 EXE 모드, worker loop 라이브러리화, native bootstrap | Linux 회귀 유지·Windows cross build·Wine 경로/IPC/GUI 초기화 검사 |
| W1 파일 안전성 | Windows file ID/변경 시각, private DACL, reparse 방어·열린 파일 sharing, temp 정리 | native/Wine 파일 교체·symlink/junction·원본 불변·오류 회귀 |
| W2 worker 보호 | Job Object memory/kill-on-close, 시작 handshake, 부모 사망·취소·reap | 보호 완료 전 decoder 진입 불가·부모 kill 시 worker 잔류 0 |
| W3 native 뷰어 | Win32 창/파일 열기·drop·paint·메시지 기반 backend wakeup, 같은 EXE `--worker` 연결 | GUI 프로세스에서 파싱하지 않고 실제 PNG/CBZ 표시 |
| W4 기능 동등성 | 페이지/RTL·회전/zoom·애니메이션·프레임 seek/pause/speed·키 입력 | synthetic/실물 Windows·Wine 회귀, idle polling 없음 |
| W5 배포 | static runtime·PE import 검사·리소스 embed·서명/고지·EXE artifact | 외부 파일 없는 깨끗한 디렉터리에서 실행·OS DLL 외 import 없음 |

기반 단계의 `--self-test`/창 초기화는 전체 viewer/worker 시험이 아니다. W1/W2 전에는 `--worker`가 명시적으로 실패해야 하며, readiness를 true로 바꾸지 않는다.

### 경로/IPC

- Unix raw bytes는 유지한다. Windows는 `encode_wide`/`from_wide`의 UTF-16 code units를 LE bytes로 전달해 한글·emoji·unpaired surrogate를 보존한다.
- Windows 경로 byte 배열은 짝수 길이여야 한다. native NUL·최대 길이·잘못된 wire 입력은 경로로 변환하기 전에 검사한다.
- native 표현의 식별 bytes와 UI label/자연 정렬용 bytes는 별도 정책이다. Windows UTF-16 LE를 UTF-8인 것처럼 label 처리하지 않는다.
- 동일 EXE worker는 명시적인 인자 배열로 실행한다. shell command·PATH 검색·책 디렉터리의 실행 파일을 사용하지 않는다.
- 같은 wire 버전의 GUI/worker만 연결한다. 다른 native path encoding을 네트워크 프로토콜이나 영구 저장 형식으로 취급하지 않는다.

## 4. Wine 시험 규칙

- 사용자 기본 `~/.wine`을 변경하지 않는다. 도구가 만든 **별도 prefix**를 사용하고 테스트 경로를 출력한다.
- source/fixture는 합성 입력이다. 사용자 이미지·네트워크 자격증명·기본 Wine registry를 fixture로 가져오지 않는다.
- prefix 초기화는 Wine 자체의 OS 호환 환경 준비다. 앱이 DLL/EXE를 추출한 것으로 계산하지 않되, 앱 배포 폴더에는 EXE 하나만 둔다.
- Wine에서는 `--self-test-platform`, `--smoke-window`, 보호 없는 `--worker` 거부를 timeout/exit status로 검사한다. 전체 `--self-test`는 보호된 worker handshake도 요구하며 Job readback 실패를 성공으로 낮추지 않는다.
- 이후에는 한글/공백/emoji 경로·folder/CBZ·frame/disposal·오래된 결과·worker timeout·부모 kill·휴면 CPU·임시물/handle 반환을 추가한다.
- 실행 전후 PE import 목록과 배포 폴더를 비교한다. Wine prefix의 `drive_c/windows` 파일들을 앱 runtime DLL로 번들링하지 않는다.
- Wine에서 Job Object/ACL/host filesystem 특성이 Windows와 다를 수 있다. Wine 통과만으로 실제 NTFS ACL·SmartScreen·IME·접근성·DPI·그래픽 인증 완료라고 보고하지 않는다.
- Wine prefix는 보안 sandbox가 아니다. 기본 Z: 매핑 등으로 host 파일에 접근할 수 있으므로 이 도구는 프로젝트의 자체 빌드 EXE/합성 입력에만 사용한다.

## 5. 빌드와 보고

GNU cross toolchain 또는 llvm-mingw와 Rust Windows target을 프로젝트 외 도구 경로에 설치한다. 호스트 시스템 패키지·기본 Wine prefix는 임의로 변경하지 않는다. target별 CRT static/PE import 검증을 적용하며 필요 없는 GTK graph를 Windows binary에 링크하지 않는다.

각 작업 보고에는 구현 단계·실행한 명령·Wine 버전/target·검사 결과·미검증 사항을 구분한다. 새 버전·커밋·푸시·공개 Release는 별도 사용자 요청 전 수행하지 않는다.

## 6. W0 당시 구현 및 재현 (이력)

이번에 구현한 범위:

- `ncat-windows`: GTK 비의존 Win32/GDI 창·blocking message loop·명시적 smoke timer. 이미지 뷰어는 아직 아니다.
- `--self-test`: 한글/emoji·unpaired surrogate 경로, bounded wire round-trip, 공통 배속 정책, 같은 EXE의 `--worker` 거부와 `--ipc-probe` child pipe 왕복.
- `--ipc-probe`는 개발용 Shutdown/Bye 검사만 지원한다. 파일을 열거나 codec을 호출하지 않으며 실제 worker 모드와 구분한다.
- protocol wire v4의 Windows UTF-16LE와 Deserialize 시 길이/NUL/alignment 검사. Linux raw byte identity는 유지한다.
- backend `WorkerCommand::same_executable()`와 기존 separate-worker 호환 API. worker loop를 `ncat-worker` 라이브러리로 옮겨 추후 동일 EXE에 연결할 수 있게 했다.
- `build-single.py`의 PE system-import 검사, `windows-wine-smoke.py`의 private prefix·한글/공백 배포 경로·단일 파일 유지 검사. 별도 Windows bootstrap CI 정의.

Linux x86_64에서의 재현 예:

```bash
rustup target add x86_64-pc-windows-gnullvm
python3 packaging/windows/fetch-toolchain.py /tmp/opencode/my-new-llvm-directory
python3 packaging/windows/build-single.py \
  --toolchain /tmp/opencode/my-new-llvm-directory/llvm-mingw-20261104-ucrt-ubuntu-22.04-x86_64
python3 tools/windows-wine-smoke.py --image target/windows-single/nCatImage.exe
```

Wine 자체는 호스트에 미리 설치되어 있어야 한다. 도구는 새 prefix와 배포 폴더를 만들고 경로를 출력하며, 사용자 prefix를 사용하거나 자동으로 삭제하지 않는다. 새 toolchain 디렉터리를 사용하고 원본 archive SHA256 pin을 우회하지 않는다. 테스트 결과/로그는 생성된 경로의 `result.json`과 `.log`에 남긴다.

Rust Windows unit test 실행 시에도 전용 prefix를 지정한다:

```bash
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=/path/to/verified/llvm-mingw/bin/x86_64-w64-mingw32-clang
export RUSTFLAGS='-C target-feature=+crt-static -C link-arg=-Wno-unused-command-line-argument'
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUNNER=wine
export WINEPREFIX=/tmp/opencode/my-dedicated-wine-unit-prefix
export WINEARCH=win64
export WINEDEBUG=-all
export WINEDLLOVERRIDES='mscoree,mshtml='
cargo test -p ncat-core -p ncat-presentation -p ncat-protocol \
  --target x86_64-pc-windows-gnullvm --locked
```

초기 확인: Wine 11.22에서 self-test/창 생성·종료/worker 거부 3개와 Windows-target 순수 Rust 21개를 통과했다. 테스트 폴더에는 EXE 하나만 남았고 PE import는 Windows 시스템/API-set DLL뿐이었다. 시스템 UCRT API-set은 여전히 사용하므로 **모든 C runtime 코드까지 완전 정적**이라고 주장하지 않는다. 현재 PE는 약 740KB의 bootstrap이며 codec/뷰어가 추가되면 크기도 달라진다.

W0 당시에는 Windows backend/media·ACL/Job 보호와 codec/GUI 연결이 미구현이었다. 이후 구현 상태는 아래 7절을 기준으로 하며 `packaging/targets.json`의 `ready: false`는 유지한다.

Linux 회귀는 Rust 85개(장기 1개 ignored)·GTK 재생/컨트롤 8개·Python 배포 검사 6개 통과, Linux/Windows-target Clippy·release/native PE 빌드·actionlint 통과다. 기존 Linux AppImage와 공개 Release는 교체하지 않았다. 이 작업에서는 버전 변경·커밋·푸시·원격 workflow 실행을 하지 않았다.

## 7. W1/W2 구현 및 W3/W4 기능 연결 (2026-11-17 후속)

### 구현한 코드

- Windows 파일 서비스: 128-bit file ID/volume/change time, owner/SYSTEM 전용 protected DACL로 **새** private 디렉터리 생성, create-new 출력, handle 기반 final reparse/type/DACL 검사 후 truncate. 기존 디렉터리 ACL을 수정하는 미사용 경로는 제거했다. 전체 ancestor/junction 공격 검증 완료를 의미하지 않는다.
- worker: 동일 EXE에 codec/ZIP 라이브러리를 링크하되 `--worker` 진입점에서만 실행한다. 부모가 non-inherited Job에 child를 할당하고 memory/active-process/kill-on-close 제한을 readback한 뒤 시작 토큰을 보낸다. 자식도 Job 제한을 확인하기 전에는 IPC/codec에 진입하지 않는다.
- Win32 뷰어: 파일/폴더 대화상자·단일 파일/폴더 drop·CLI source, 자연 정렬 catalog·이전/다음/번호 탐색·RTL, fit/zoom/pan/회전·전체 화면·UI 숨김, 재생/일시정지·이전/다음 프레임·번호 seek·0.25×–4× 배속을 공통 backend/presentation에 연결했다.
- GDI BGRA staging은 UI 밖에서 수행한다. 현재 frame의 `Arc<Frame>` lease를 유지하며 세대 검사·bounded bridge·메시지 wakeup을 사용한다. 종료는 backend shutdown 요청과 한시적인 shutdown timer이며 idle polling은 없다. 초기/오류 경로에도 HWND가 callback state보다 먼저 해제되도록 guard를 둔다.
- 주소: literal Windows/UNC 경로, percent-encoded UTF-8 `file://`/`smb://`를 처리한다. URI의 자격증명·query/fragment·잘못된 encoding/control 문자를 거부하고 shell을 실행하지 않는다.

### 실제 실행한 검사

| 검사 | 결과 |
|---|---|
| Linux `cargo test --workspace --locked` | 90개 통과, 수동 장기 1개 ignored |
| Linux workspace fmt·Clippy `-D warnings`·release build | 통과 |
| 실제 Linux GTK 애니메이션 7종 + 컨트롤 1종 | 8개 통과, 기존 프레임 번호/배속/버튼 안정성 유지 |
| Python `python3 packaging/test_pipeline.py` | 6개 통과 |
| Windows gnullvm cross-target Clippy (backend/media/platform/worker/GUI 포함) | 통과 |
| Windows-target unit tests, Wine 11.22 | 30개 통과, native symlink 권한 필요 검사 1개 ignored |
| Wine `--self-test-platform`·빈 GUI 창·보호 없는 worker 거부 | 3개 통과; 새 private prefix, 배포 폴더에는 EXE 하나 |
| cross PE build + import 검사 | 통과, 약 2.6 MB; GTK/third-party DLL 없이 system/API-set DLL만 |
| 전체 protected worker self-test / PNG·CBZ GUI | Wine Job readback 제약으로 실패/차단; 성공으로 계산하지 않음 |

Windows unit test의 codec 검사는 **직접 생성한 신뢰 fixture 전용 library test**다. PNG/JPEG/BMP/WebP·CBZ·회전·원본 불변·GIF next/seek가 Wine에서 통과했지만, GUI가 보호를 우회하거나 protected pipeline이 통과했다는 의미가 아니다.

재현 시 6절 unit-test 명령에 `-p ncat-platform -p ncat-media -p ncat-windows`를 추가한다. symlink privilege가 있는 native Windows에서만 다음 검사를 명시적으로 실행한다:

```powershell
cargo test -p ncat-platform final_reparse_is_rejected_before_truncation --locked -- --ignored
cargo build -p ncat-windows --release --locked
python tools/windows-native-smoke.py --image target/release/ncat-windows.exe
```

`windows-native-smoke.py`는 전체 `--self-test`, PNG/CBZ protected pipeline, GUI frame/animation/500회 탐색을 실행하고 EXE/원본 hash와 배포 폴더를 확인한다. CI `native` job은 정의만 추가했으며 **실제 Windows 실행 결과는 아직 없다**. CI 정의와 성공 기록을 혼동하지 않는다.

### 확인된 blocker와 남은 동등성

- Wine 11.22는 `SetInformationJobObject` 성공 후 parent explicit-handle/child NULL-job readback에서 flags/memory/processes를 0으로 반환한다. 따라서 보호된 worker 시작을 거부한다. 검증을 끄거나 in-process decoder fallback을 추가하지 않았다.
- W1/W2 완료 판정에는 native NTFS ACL·junction/reparse·교체 공격, Job 메모리 고갈·부모 crash/worker 정리·취소/handle 반환 검증이 남아 있다. Job은 filesystem sandbox가 아니다.
- W3/W4는 코드 연결 단계다. native PNG/CBZ 실제 표시·전체 애니메이션/disposal·실제 keyboard/mouse·IME/접근성·DPI/작은 창 layout·fullscreen/resize 품질을 검증해야 한다.
- FTP/SFTP 직접 OS 연결 서비스는 미구현이다. 현재 OS에서 미리 연결한 drive 경로만 열 수 있다. Ctrl+L은 fullscreen을 해제해 inline 주소란을 보이므로 Linux modal 주소 입력과 아직 동등하지 않다.
- W5 리소스/icon·서명·배포 파일 연결·깨끗한 Windows 실행 검증은 남아 있다. 개발 PE는 `target/windows-single/nCatImage.exe`; 공개 제품이 아니며 Linux AppImage/Release는 그대로다.
