# 08. 실제 구현 상태와 Linux 검증 기록

기준일: 2026-11-16 · v0.5.0 · **M0 일부 실험 + M1 개발 프리뷰, MVP/정식 안정판 아님**

## v0.5.0 배포

사용자 승인으로 Linux AppImage를 우선 배포한다. 아래 미릴리스로 기록된 포팅 분리·애니메이션·프레임 UI 변경은 v0.5.0에 포함된다. Windows/macOS는 현재 native 파일 API/worker 보호 미구현으로 빌드 사전 검사에서 차단되며 공개 실행 파일은 없다. 기존 v0.4.3 Release는 보존한다.

## 0. v0.4.3 최신 작업 요약

### 미릴리스: 포팅 준비 리팩터링

#### 애니메이션 후속 구현

- GIF/AGIF·WebP·APNG 순차 자동 재생을 worker→backend→GTK에 연결했다. 별도 APNG default 이미지 제외, 합성/disposal·지연·유한/무한 반복을 처리한다. `.png` APNG와 ZIP/CBZ 내부 입력도 같다.
- wire v2, parent 수명의 persistent workspace 예약, 프레임 한 개씩 요청하는 one-shot timer를 사용한다. 정지 cache/prefetch와 분리하며 이전 source/view/close의 재생을 무효화한다.
- 프레임 UI 후속 변경: 타이틀바 프레임/총수·상태·배속, 이전(`,`)/다음(`.`)/재생·일시정지(`/`) 버튼과 0.25×~4× dropdown을 추가했다. 총수는 주기 완료 후 확인하며 역방향은 bounded worker 재합성이다. finite 종료 후 세션을 유지해 이전/재시작을 지원하고 취소·페이지 변경·종료에서 해제한다.
- Rust 테스트 **80개 통과**, 기존 수동 장기 1개 ignored. 신규 애니메이션 통합 10개는 색/지연/반복·disposal·예산/원본/lease·ZIP/회전·취소·손상 후 복구·이전 seek/종료 후 재시작을 검사한다. 키 정규화·modifier/focus 우선순위·배속/프레임 총수 정책도 단위 검사한다. fmt·Clippy·release 빌드·actionlint가 통과했다.
- 최종 release 빌드의 실제 Linux GTK(Cairo)에서 GIF/AGIF/WebP/APNG/PNG/CBZ와 단발 GIF 재생 7개 및 scripted GTK 컨트롤(일시정지 유지·이전/다음·2배속·종료 후 재시작·타이틀) 검사 **8개 모두 통과**했다. 실물 corpus·정확한 compositor FPS·원격 재생·Windows/macOS 및 실제 OS 키 입력 주입은 미검증이다.
- 최초 애니메이션/UI 구현 후 후속 사용자 요청으로 커밋·푸시·workspace release 빌드와 Linux AppImage 패키지 작업을 실행한다. 버전은 유지하고 기존 GitHub v0.4.3 Release/태그는 덮어쓰지 않는다. 새 소스 기반 Actions 패키지와 과거 0.4.3 정지-only 배포본을 구분한다.

- 후속 배포 요청으로 `ncat-desktop` 로컬 진입점을 추가해 현재 workspace는 10개 crate다. AppImage/Windows portable ZIP/macOS arm64 DMG workflow와 스크립트를 추가했다. Windows/macOS는 readiness gate에서 차단된다. 정의 추가와 실행 가능한 세 OS 배포 완료를 구분한다.
- 배포 준비 로컬 검증: Rust 65개(장기 1개 ignored), Python release gate/체크섬 테스트 5개, fmt·Clippy·workspace release 빌드·actionlint 문법 검사 통과. native Windows/macOS 패키징/실행은 아직 미검증이다.
- 이전 작업의 “커밋·푸시·빌드 미실행” 상태와 달리 이번에는 사용자 요청에 따라 변경 전체를 커밋·푸시하고 수동 Distribution packages 작업을 실행한다. 새 버전/태그/공개 Release는 만들지 않는다. 실행 결과는 작업 보고 및 Actions 기록을 기준으로 한다.

- 공통 GUI 명령/보기 정책 `ncat-presentation`, GTK frontend `ncat-gtk`, OS 파일/보호 구현 `ncat-platform`을 추출해 workspace를 9개 crate로 나눴다. Linux 진입점은 KIOFuse/GVfs 연결 서비스를 주입한다.
- 기존 Unix 경로 bytes, device/inode/length/mtime/ctime 비교, private 파일 권한, no-follow 출력, Linux worker 보호를 유지한다. Windows/macOS 앱·패키징은 미구현이며 포팅 범위는 [09](09-portability.md)에 명시한다.
- 이번 Linux 검증: fmt·Clippy·workspace release 빌드·Rust 테스트 **65개 통과, 수동 장기 시험 1개 ignored**. 아래 56개는 배포된 0.4.3의 과거 결과다.
- 현재 환경에 Xvfb/xdotool/Openbox와 이전 FTP 테스트 가상환경이 없어 desktop/모의 네트워크/실제 FTP 회귀를 재실행하지 못했다. 과거 통과를 이번 리팩터링의 GUI 통과 결과로 계산하지 않는다.
- 설치된 Rust target은 Linux만이며 Windows/macOS compile/실행은 미검증이다. 최초 분리 단계에서는 커밋·푸시·패키지 갱신을 하지 않았으며, 후속 배포 요청의 실행 상태는 위 기록과 Actions 결과로 구분한다. 기존 Release는 유지한다.

### 배포된 v0.4.3

- AI 코딩 에이전트를 통한 설계·구현·검증·문서 개발 사실을 README에 명시했다. 전체 이력은 [CHANGELOG](../CHANGELOG.md)에 기록한다. AI 출력·자동 검사를 독립 보안/품질 인증으로 표현하지 않는다.
- 0.4.0의 Rust/GTK/worker/캐시 수직 경로에 0.4.1의 상단 UI·백틱 토글·전체화면 숨김·클릭 후 입력·↑/↓·Esc 종료를 추가했다. 0.4.2에서 이름·앱 ID·패키징을 nCatImage로 통일했다.
- 0.4.3은 드롭 형식 수신 보강과 진단, OS 경유 SMB/FTP/SFTP 연결, Ctrl+L/네트워크 버튼, 비동기 연결·이전 요청 취소·60초 대기 제한을 포함한다. HTTP/WebDAV/FTPS·자체 계정/다운로드 관리자는 범위 밖이다.
- 사용자 Wayland 진단은 두 번의 `Received → Rejected(NonLocal)`과 포털 경유 로컬 경로의 `OpenQueued → Displayed`를 확인했다. X11 표준 URI 드롭은 기존 빌드에서도 성공했으므로 모든 드롭 실패를 한 가지 형식 문제로 단정하지 않는다.
- KIOFuse에서 실제 익명 FTP 폴더·CBZ 열기와 각각 500회 탐색을 확인했다. 첫 읽기 중 stat 갱신으로 최초 열기가 Changed를 반환하는 경우를 확인했고, catalog/frame 게시 전 새 worker로 한 번만 다시 연다. 이후 리비전 검사는 유지한다.
- 기본 Rust 테스트 **56개 통과 / 수동 장기 시험 1개 ignored**, fmt·Clippy·workspace release 빌드가 통과했다. 저장소에 worker 통합 테스트·예제·합성 입력/측정 도구와 재현 가능한 desktop/네트워크 검증 코드를 포함한다.
- Xvfb/Openbox X11에서 실제 키·마우스·로컬/원격 URI 드롭·모의 KIOFuse·Ctrl+L 입력·취소·늦은 결과 무시·대기 중 종료·비식별 로그를 검사했다. KDE Wayland에서 실제 FTP와 로컬 CBZ smoke를 확인했다. **SMB/SFTP 실서버 인증·GVfs end-to-end·실제 Ubuntu GNOME·한글 IME·접근성 전체는 미검증**이다.
- AppImage는 Ubuntu 24.04에서 GTK 4.14.5와 함께 빌드하며 glibc 2.39 이상을 요구한다. GUI/worker·GTK·아이콘·schemas·고지를 포함하고 Cairo를 기본으로 한다. FUSE 일반 실행과 압축 해제 실행, 시스템 GTK/아이콘을 가린 X11 실행을 확인했다. 코드 서명·완전 static·전체 배포판 호환을 보장하지 않는다.
- 진단은 시작/드롭/연결/장애 시 자동 snapshot이며 GUI 대기 32개·이력 128줄·줄당 1KiB·최근 관리 파일 8개·권한 0600이다. 경로·파일명·URI·서버·계정·비밀번호·이미지 내용을 남기지 않는다. 원본 사용자 ZIP과 실제 로그는 공개하지 않는다.
- 네트워크는 OS 도구의 인증·FUSE·캐시에 의존한다. 외부 데몬의 메모리/전체 파일 캐시는 앱의 192MiB 예약에 포함되지 않는다. 아래 과거 메모리/지연 표를 네트워크 성능이나 0.4.3 정식 corpus 결과로 재해석하지 않는다.

## 1. 구현한 수직 경로

`ncat-linux(GTK) → ncat-backend(조정 thread) → ncat-worker(별도 프로세스) → ncat-media(코덱/ZIP) → 부모 소유 RGBA lease → GdkMemoryTexture`.

- Rust Cargo workspace 6개 crate·toolchain/lock·Linux CI 정의를 추가했다. 모든 first-party crate가 unsafe를 금지한다. worker 보호와 no-follow open은 rustix 안전 API를 사용한다(의존성 내부 unsafe까지 없다는 의미는 아니다).
- 파일/폴더/ZIP/CBZ 열기·worker 내 자연 정렬/메타/디코딩·선택 파일 우선 표시 후 폴더 목록·이전/다음/번호 이동·일부 keyboard/드롭/회전/맞춤/확대/패닝/전체 화면이 연결되었다.
- 실제 GUI는 단일 페이지다. RTL은 현재 좌우 키 매핑만 바꾸며 spread 생성 알고리즘은 core 단위 테스트까지다.
- 한 worker를 지연 실행/재사용한다. UI 조정·IPC reader thread, worker 실행 thread이며 Tokio/Rayon/libadwaita/custom renderer를 도입하지 않았다.
- 제어 IPC는 버전·요청 ID·256KiB 길이 제한, 큰 pixels는 private 임시 파일→부모 소유 buffer로 복사한다. 원본 항목명으로 추출하지 않는다. 임시 buffer는 정상/오류/취소 시 RAII로 제거한다.
- 기본 192MiB 예약은 book index·진행 workspace·표시 buffer/추정 GUI staging을 합산한다. Arc lease를 GBytes owner가 유지한다. 예약 수치는 전체 RSS와 다르다.
- 소스/선택 변경은 generation을 증가시키고 오래된 결과는 UI에서 버린다. 진행 중 취소 불가 작업은 worker kill/reap 후 다음 요청을 연다. stdout/stderr/오류에 원본 경로를 내보내지 않는다.

## 2. 기능 ID별 현재 상태

| ID | 현재 구현 | 남은 부분 |
|---|---|---|
| R01 | 대화상자·명령행·FileList/URI/GFile 단일 드롭·ZIP/CBZ·OS 경유 SMB/FTP/SFTP·다중 드롭 거부 | desktop/MIME 전체·SMB/SFTP 실제 인증·GVfs 전체·RAR |
| R02 | 자연 정렬·이전/다음/첫/끝·번호·다시 열기 | 검색·역순·새로고침의 선택 유지 |
| R03 | 좌우 키 RTL/LTR, core spread 함수/테스트 | 실제 두 장 UI·표지/가로 메타 조정 |
| R04 | 전체 맞춤·중앙 확대 preview·native scroll/drag pan | 폭/높이/1:1·pointer anchor·최소 줌/소형 확대 옵션·고해상도 업그레이드/타일 |
| R05 | JPEG/PNG/BMP/WebP 정지·GIF 및 미릴리스 GIF/AGIF/WebP/APNG 재생·ZIP/CBZ 합성 입력 회귀 | ICC/CMYK 정확성·EXIF 전체 corpus·RAR·실물 형식/방법별 인증 |
| R06 | 화면 회전·파일 불변 테스트 | 추가 감상 변환은 범위별 별도 구현 |
| R07 | 파일 이름/번호·상단 조작·백틱 토글·전체화면 숨김/복귀 | 시간 기반 auto-hide·thumbnail strip·전체 UX 정리 |
| R08 | 기록을 전혀 저장하지 않음 | opt-in/최근 책/이어보기·원자 저장/삭제 전체 |
| R09 | 예약/RAII·IPC/최신 요청·설정 가능 deadline·kill/reap·비동기 종료·LRU/인접 prefetch·동일 대상 진행 작업 합류 | 일반 다중 구독 작업 공유·협력 취소·유휴 책 닫기·강한 worker sandbox |
| R10 | 클릭 후 native Tab/Enter·한국어 label·↑/↓·Esc·Ctrl+L 및 기존 shortcuts | 클릭 전 Tab 차단의 접근성·실물 IME/Orca·도움말·터치 검증 |
| R11~R17 | 미구현 | 01의 후속 단계 유지 |

## 3. 현재의 중요한 한계

1. **JPEG 축소 디코딩을 아직 쓰지 않는다.** 모든 형식에 보수적인 full-decode workspace를 예약하고 한 작업은 기본 최대 128MiB workspace다. 예산/cache/workspace/deadline/prefetch는 README의 검증된 환경변수로 설정 가능하지만 설정 UI는 없다. 큰 이미지는 기본 예산보다도 일찍 거부될 수 있다.
2. ICC 변환/linear-light·premultiplied resize 정확성은 미완료다. GUI에 ICC 미구현 상태를 표시하며 색상 정확성이 필요한 용도로 인증하지 않는다.
3. 출력은 최대 8MiB인 preview 한 개다. 4K/높은 zoom을 실제 원본 화질로 완성하는 타일/crop 경로는 아직 없으므로 1:1/최종 품질로 홍보하지 않는다.
4. 폴더의 현재 디렉터리만·ZIP/CBZ에 기본 24MiB 픽셀/키 LRU(최대 64개), 진행 방향 인접 페이지 1개 prefetch를 적용한다. key는 book epoch/페이지 ID/소스 stat 리비전/보기 크기/회전이다. 전체 책 선행 해제/원본 캐시는 없다. 회전 중 prefetch는 보수적으로 끈다. 다른 대상 선택 시 진행 worker를 회수하고 동일 대상 요청은 합류한다. 문서의 협력 유예/취소 reader·비가시 자동 중단·압력 hysteresis는 아직 미구현이다.
5. 제어 결과 큐는 현재 4개, worker reader 큐는 2개이며 terminal 전용 슬롯/일반 다중 구독 작업 공유는 아직 없다. GUI는 결과마다 main context에 양보하고 steady-state polling은 하지 않는다. 종료만 유한 10ms 확인을 사용한다.
6. ZIP central directory의 라이브러리 내부 메모리·codec metadata 내부 선행 할당은 완전 계측/제한하지 못했다. index 비용/전체 decoder workspace는 보수적인 예약이며 각 native/driver 실제 할당의 증명은 아니다.
7. worker는 Linux RLIMIT_AS 1GiB·core dump 0·parent-death kill·no-new-privileges 적용 실패 시 실행을 거부한다. **파일 접근/네트워크/seccomp sandbox는 아직 없다.** 별도 프로세스를 완전한 보안 격리라고 주장하지 않는다.
8. 개발 앱 ID는 `io.github.ncatimage.Development`이며 AppImage·소스/고지 수집을 제공한다. 정식 라이선스·고지/소스 의무 검토·서명·RustSec/native 취약점 audit·지원 corpus는 미완료다. `publish=false`이며 개발 프리뷰를 안정판으로 취급하지 않는다.

## 4. 실행/재현

```sh
cargo build --workspace --release --locked
./target/release/ncat-linux /path/to/book.cbz

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p ncat-worker --test pipeline ten_thousand_decodes_return_all_leases --locked -- --ignored
```

합성 smoke 입력은 아래 명령으로 **아직 없는 디렉터리**에 생성한다. 사용자 이미지·외부 corpus를 다운로드하지 않는다.

```sh
python3 tools/smoke-fixtures.py /tmp/opencode/ncat-fixtures
NCAT_WORKER="$PWD/target/release/ncat-worker" cargo run --release --locked -p ncat-backend --example inspect -- /tmp/opencode/ncat-fixtures/page1.png
python3 tools/measure-smoke.py -- target/release/ncat-linux --smoke-test /tmp/opencode/ncat-fixtures/sample.cbz
python3 tools/measure-smoke.py -- target/release/ncat-linux --smoke-empty
```

`--smoke-test`는 정상 texture가 만들어진 뒤 잠시 표시하고 자동 종료한다. `--smoke-empty`는 빈 창의 map/종료만 확인한다. 실제 제시 지연·화질·입력/접근성 인증을 대체하지 않는다. `/tmp/opencode`가 없는 머신에서는 사용자가 소유한 임시 디렉터리를 선택한다.

## 5. 현재까지 확인한 결과

이 절의 하드웨어·메모리·decode/cache 수치는 **2026-11-15 초기 실험 기록**이다. 최신 기능과 56개 테스트·네트워크 검증은 위 0절을 기준으로 하며 과거 숫자를 조용히 덮어쓰지 않는다.

- 당시 로컬: **CachyOS rolling x86_64 / KDE Wayland / Ryzen 5 5600X / 약 32GiB RAM**. Rust 1.98.1·GTK 4.24.1·GLib 2.90.1. 이후 Ubuntu 24.04 기준 빌드/Xvfb X11 검증을 추가했다. 실제 GNOME·Orca·터치와 원격 CI 결과는 개별 실행 기록을 확인한다.
- image 0.25.10(기본 feature 끔: jpeg/png/gif/webp/bmp), zip 9.6.0(Deflate zlib-rs 경로), gtk4-rs 0.11.5(v4_10), async-channel 2.5.0. 실제 잠금은 Cargo.lock이다. 최소 Rust 1.92 선언 자체는 해당 컴파일러 검증 기록이 아니다.
- workspace fmt/check·Clippy `-D warnings`·**기본 테스트 43개**·release build가 로컬에서 통과했다. 별도 확장 수명 시험 1개는 기본 실행에서 ignored이며, 명시적으로 cache/prefetch를 끄고 실행해 **10,000 실제 decode 후 예약 잔류 0**을 확인했다. GitHub Actions 파일은 추가했지만 원격 CI 실행을 확인한 것은 아니다.
- 수명 연장 시험은 **합성 4×6/6×4 PNG의 headless worker decode 10,000회**다. 실제 GUI/200페이지 corpus의 10,000회 plateau/벤치마크 게이트와는 다르다.
- 초기 실제 Wayland CBZ smoke는 정상 texture/자동 종료를 확인했다. GTK 시스템 테마에서 slider 최소 크기 경고가 발생했으며 테마/배율별 수동 검증이 남아 있다. 07의 목표 수치는 이 단발 실행으로 합격 처리하지 않는다.

### 단발 실제 GTK smoke — release, KDE Wayland

최종 생성 입력은 800×1200 두 장·1200×800 한 장의 합성 PNG/CBZ다. **각 0.96MP**로 정식 기본 2~4MP/200페이지 corpus보다 작다. 아래는 10ms 주기의 프로세스 트리 샘플 peak이며 안정 상태·실제 첫 제시·p95·유휴 CPU 측정이 아니다. 전체 실행 시간에는 준비/자동 종료 지연이 들어간다. 따라서 07의 100MiB 빈 창 안정 목표나 1초 첫 표시 목표의 합격/실패로 곧바로 환산하지 않는다.

| 실행 | 종료 | 프로세스 최대 | 샘플 peak RSS 합산 | 샘플 peak PSS 합산 | 전체 smoke 시간 |
|---|---|---:|---:|---:|---:|
| 기본 GTK renderer·빈 창 | 정상 | 1 | 145,852KiB | 78,164KiB | 849.46ms |
| 기본 GTK renderer·PNG | 정상 표시/종료 | 2 | 161,852KiB | 91,819KiB | 456.93ms |
| 기본 GTK renderer·CBZ | 정상 표시/종료 | 2 | 161,732KiB | 91,752KiB | 454.88ms |
| `GSK_RENDERER=cairo`·빈 창 | 정상 | 1 | 63,864KiB | 32,355KiB | 612.78ms |
| `GSK_RENDERER=cairo`·CBZ | 정상 표시/종료 | 2 | 81,224KiB | 42,587KiB | 443.29ms |

기본 renderer의 driver/공유 라이브러리 비용과 소프트웨어 경로 차이가 커서 다음 M0에서는 renderer별 안정 상태·CPU·4K/zoom·실제 frame 간격을 비교한다. Cairo를 무조건 더 빠르거나 모든 Linux에서 더 작다고 결론 내리지 않는다. 이 실험 당시 renderer 기본값은 바꾸지 않았지만 이후 AppImage는 호환성을 위해 Cairo를 기본으로 정했다. 앞선 최초 실행은 shader/cache 등 조건 차이로 더 큰 peak/긴 시간이 있었으며 위 단발 결과를 재현 통계로 일반화하지 않는다.

별도 headless PNG inspect 한 번에서 `800×1200 / 3,840,000 pixel bytes / decode 4,352µs / 요청→수신 8,936µs / 예약 36,685,824 bytes`를 확인했다. 이는 **GUI 제시 latency가 아니고** 같은 때 다른 빌드가 진행 중이었으므로 성능 비교값으로 쓰지 않는다.

생성기 재실행의 byte 일치를 확인했다. 재현 CBZ SHA-256은 `90912d595eac1225a5e4d96fc35b42c6466b19797a2c90891d9cca8a05ddbae5`다. ZIP timestamp도 고정되어 있다. 정식 corpus hash/환경 manifest/30회 측정은 별도 작업이다.

### 캐시 추가 후 재검증

- 폴더 하위 디렉터리 제외·폴더/ZIP 프레임 재사용(Arc 동일성)·prefetch hit·소스 변경/삭제·재열기·보기/회전 분리·LRU 퇴출/표시 lease 유지·64MiB에서 전경 우선 회수·동일 대상 작업 승격/다른 대상 취소·설정 검증·설정 deadline/worker 회수를 회귀 테스트에 추가했다.
- 모든 first-party crate에 `forbid(unsafe_code)`. 직접 libc 의존성을 rustix 1.1.6의 safe API로 교체했다. 보호 적용 실패는 계속 실행 거부하며 상속된 더 낮은 주소 공간 제한은 올리지 않는다. 의존성과 시스템 라이브러리 내부 unsafe는 별도다.
- 새 release의 실제 KDE Wayland PNG/CBZ texture 생성/종료 smoke도 정상이다. 기존 GTK slider 최소 크기 경고는 남아 있다. 동시 실행한 단발 smoke라 기존 메모리 표와 성능 비교하지 않는다.

동일 0.96MP 합성 PNG 3장/Deflate CBZ, release, prefetch 끔, 각 모드 독립 backend·warm-up 3회 제외 후 **500회 재탐색 × 3묶음**. OS file cache를 비우지 않았고 순서는 매번 cache off→on이다. p95는 정렬된 500개 중 475번째(nearest rank), 아래 범위는 3묶음의 최소~최대다. **GTK upload/compositor 제시를 포함하지 않는 요청→backend 이벤트 수신 시간**이며 정식 200페이지/2~4MP 게이트가 아니다.

| 소스 | cache off p50 / p95 | cache on p50 / p95 | 측정 중 최대 off / on |
|---|---|---|---|
| 폴더 | 2,470~2,551 / 3,143~3,232µs | 5~6 / 7~8µs | 4,284 / 75µs |
| CBZ | 2,433~2,504 / 3,122~3,225µs | 5~6 / 7~9µs | 4,853 / 122µs |

각 묶음 전체 504요청(첫 열기+warm-up 포함)에서 off는 504 decode, on은 3 decode/501 hit, worker 시작은 양쪽 모두 1회, cache는 약 11MiB였다. 캐시 적중은 같은 Arc 픽셀을 반환하므로 디코딩/임시 출력/IPC 픽셀 복사를 건너뛰지만 GUI texture 생성 비용은 남는다.

```sh
NCAT_WORKER="$PWD/target/release/ncat-worker" cargo run --release --locked -p ncat-backend --example cache_bench -- /tmp/opencode/ncat-cache-fixtures
NCAT_WORKER="$PWD/target/release/ncat-worker" cargo run --release --locked -p ncat-backend --example cache_bench -- /tmp/opencode/ncat-cache-fixtures/sample.cbz
```

입력은 `tools/smoke-fixtures.py`로 해당 새 디렉터리에 생성한다. 실제 큰 책/빠른 연속 이동에서 prefetch 비용과 GUI 제시 지연 검증은 남아 있다.

### 사용자 제공 대용량 ZIP / 빠른 이동 오류 조사

- 원본은 로컬 테스트 입력으로만 사용하며 파일명/항목명/이미지/해시는 문서나 공개 회귀 입력에 넣지 않는다. 크기 **1,273,146,474 bytes(약 1.19GiB)**, 전체 4,422항목, 읽기 대상 4,284페이지. 원본을 수정하거나 전체 선행 압축 해제하지 않았다. 시험 전후 stat와 SHA-256 일치를 확인했다.
- 수정 전 release backend에서 0/1/5/10/30ms 간격의 빠른 이동을 시험했지만 보고된 Changed 오류는 재현되지 않았다. **해당 사용자 오류의 원인은 미확정**이다. 원본 파일이 실제로 변경되었다고 단정하지 않는다.
- 확인한 취약한 오류 경로는 worker의 Changed 응답을 부모 원본 재확인 없이 바로 사용자에게 전달하는 것이었다. 원본 key가 그대로일 때만 worker kill/reap→세션 재열기→한 번 재시도하고, 실제 리비전 변경은 계속 거부한다. 재시도 후에도 원본이 그대로인데 worker가 Changed를 반환하면 Protocol 오류로 구분하며 무한 재시도하지 않는다. `source_reopens` 카운터로 복구 여부를 구분한다. 이 보강이 보고된 오류의 원인임을 증명한 것은 아니다.
- fault-injection 회귀는 변경되지 않은 원본에 worker Changed 응답 주입→복구, 반복된 모순 응답→1회 한도/Protocol 오류, 실제 ZIP 변경→Changed 거부, 합성 ZIP의 빠른 탐색→최신 페이지/원본 보존/예약 회수를 검증한다.
- `--smoke-navigation`은 실제 GTK에서 5ms 타이머로 500회 이동 요청 후 최신 texture를 확인하고 5초 유지·종료한다. 정상 읽기에는 타이머가 추가되지 않는다. backend 전용 `navigation_stress` 예제는 원본 경로/항목명을 출력하지 않고 요청 속도를 바꿔 검증한다. 빠른 이동은 중간 요청을 병합하므로 500개 요청이 모두 디코딩되었다는 뜻은 아니다.

같은 사용자 ZIP·기본 설정·기본 GTK renderer·KDE Wayland에서 독립 GUI 3회(각 500회 요청)를 순차 실행했다. 아래는 10ms 목표 간격의 **프로세스 트리 샘플 peak**이며 정확한 순간 최대값/안정 메모리/프레임 latency는 아니다. 원래 오류는 세 번 모두 재현되지 않았고 `source_reopens=0`이므로 새 복구 경로로 오류를 해결한 실측이라고 해석하지 않는다.

| 실행 | 결과 | peak RSS 합산 | peak PSS 합산 | 최대 프로세스 |
|---|---|---:|---:|---:|
| 1 | 최신 표시·종료 성공 | 197,496KiB | 127,421KiB | 2 |
| 2 | 최신 표시·종료 성공 | 194,868KiB | 124,831KiB | 2 |
| 3 | 최신 표시·종료 성공 | 194,784KiB | 124,802KiB | 2 |

예약 사용량은 192MiB 한도 안이었고 마지막 cache 픽셀/키는 약 22MiB였다. 이 입력에서 5ms 연속 요청은 worker 시작/재시작이 141~160회로 많았다. 이는 메모리 누수/원본 변경이 아니라 현재 즉시 kill/reap 취소 정책의 비용이며 협력 취소 최적화는 남아 있다. 기존 GTK slider 최소 크기 경고도 남아 있다. 전체 4,284페이지 표시·파일 갱신 감시·장시간 plateau를 통과한 기록은 아니다.

## 6. 다음 작업

`ncat-backend::diagnostics`는 현재 디렉터리의 `log/`에 시작 준비·드롭·네트워크 연결과 원본 변경/워커/전경 오류·복구 단계 snapshot을 남긴다. 일반 탐색 이력은 메모리 128개로 제한하며 GUI 이벤트 큐는 최대 32개, 디스크 I/O는 coordinator에서 수행한다. 줄당 1KiB·관리 파일 8개·0600과 다른 파일/심볼릭 링크 보존·저장 실패 시 읽기 계속 정책을 검증했다. URI/서버/자격증명도 기록하지 않는다. 06의 opt-in 상세 logger 전체/3일 retention은 여전히 미구현이다.

M0 잔여 우선순위는 (1) JPEG 축소 API/색관리·메모리 비용, (2) Ubuntu/renderer/작은 RAM 기준 corpus·UI 제시 계측, (3) RAR과 worker 제한/라이선스 PoC다. 그 후 M1 ViewPlan/고해상도·prefetch 비가시/압력 정책을 보강하고 M2 실제 두 장/기록/검색/접근성으로 확장한다. 단순 기능 수를 늘리려고 목표/제한을 숨기지 않는다.
