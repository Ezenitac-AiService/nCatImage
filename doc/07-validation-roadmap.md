# 07. Linux 검증·성능 목표·개발 순서

v0.4.3 · 2026-11-16. 기본 Rust 회귀 56개와 별도 수동 장기 테스트 1개 ignored, GUI·드롭·모의 네트워크·실제 FTP 검증 범위는 08 및 [도구 안내](../tools/README.md)를 따른다.

## 1. 현재 상태와 판정 원칙

현재 Cargo workspace·Linux GTK/worker 수직 경로와 일부 M0 실험/회귀 테스트가 있다. 실제 결과는 [08](08-implementation-status.md)에 기록한다. 아래는 여전히 **초기 검증 목표**이며 M0 전체·MVP 게이트 달성 기록이나 보장 수치가 아니다.

Linux x64만 구현·실행 검증한다. GUI/codec이 빨라졌다고 느끼는 것과 실제 제시 지연/프로세스 메모리는 구분한다. Rust/GTK 채택 자체를 성능/메모리 우위로 주장하지 않는다.

## 2. 기준 환경과 corpus

| 구분 | 계획 환경 |
|---|---|
| 기준 | Ubuntu 24.04 LTS x64·GNOME Wayland·SSD·8GiB·4core/8thread 이상·1920×1080 SDR |
| 추가 Linux 세션 | X11·KDE Wayland·Orca·한글 IME·드롭/portal·desktop 연결 |
| 보조 부하 | 4GiB 장비 또는 제한 환경·저성능 GPU·4K/150~200%·fractional scaling |
| 모드 | 기본 192MiB, 절약 64MiB, 큰 이미지 384MiB 각각 결과 분리 |

M0에서 실제 기기를 지정하고 CPU/GPU/driver·OS/compositor·GTK/GLib·codec 버전·rustc/LLVM·release profile/feature·전원·DPI·창 크기·commit을 기록한다. 제한된 container를 실제 4GiB 기기와 같은 결과로 부르지 않는다. 현재 AppImage는 Cairo를 기본으로 하며 GTK 4.10+ API, Ubuntu 24.04/glibc 2.39 기준이다. 실제 Ubuntu GNOME 데스크톱 검증은 Xvfb로 대체하지 않는다.

### 재현 corpus

- 기본 코믹: JPEG/PNG/WebP 200페이지·각 2~4MP·불투명/alpha·EXIF 변형. 공개 가능한 파일/생성기·hash·전체 bytes를 고정한다.
- 같은 만화의 폴더/ZIP Stored/Deflate/RAR4/5·solid/비solid, P1 7Z/TAR. 기본 압축법 corpus는 기본 모드에서 **표시 실패 0**이 목표다.
- 추가 이미지: 12/24/48/100/150MP·긴 페이지·16bit/float·CMYK/ICC·잘못된 stride·TIFF 선택 page 크기 차이·AVIF/JXL 등 P1 조합.
- 읽기: 한/두 장·표지·RTL·가로 단독·분할 반쪽·연속 보기·unknown metadata·이어보기 revision 변경.
- 대형/악성: 10,000/100,000 entry·중복/긴/비UTF-8 이름·oversized index·압축 폭탄 경계·암호화/분할·truncated 데이터·worker crash/hang.
- 큰 원본/비정상 corpus는 기대 성공/예산 거부/미지원 방식을 항목별로 지정한다. 안전한 거부를 crash와 같은 실패로 세지 않되 지원 성공률도 따로 공개한다.

## 3. 측정 규약

- release 빌드/정해진 feature로 실행한다. debug와 타 앱 release를 비교하지 않는다.
- 콜드: 새 프로세스/앱 cache 없음. OS file cache를 비웠는지는 별도 표시한다. 첫 표시에는 worker 시작·압축 index·decode·IPC·texture upload를 포함한다.
- 첫 preview와 정상 표시는 분리한다. 정상은 viewport에 충분한 품질의 올바른 page/spread가 제시된 때다. 임시 저해상도를 정상으로 계산하지 않는다.
- 웜: prefetch hit/miss·앞/뒤·무작위 점프·solid/non-solid를 분리한다. 선택 상태 업데이트와 실제 pixels 제시를 각각 잰다.
- GTK texture 생성/속성 할당 시각을 실제 화면 제시로 대체하지 않는다. frame clock/compositor/외부 계측이 어려우면 대체 지표임을 명시한다.
- 콜드 최소 30회·탐색 500회 이상·독립 3묶음으로 p50/p95/max·실패율·편차·p95 산출법을 보고한다. 목표 경계면 표본을 늘려 재시험한다.
- 메모리는 GUI+모든 worker의 RSS/PSS·private/anonymous·swap·peak를 측정하고 공유 mapping RSS 중복을 설명한다. GPU/VRAM 추정·staging/live bytes는 별도 보고한다.
- 안정 상태는 마지막 작업/업로드 후 5초 대기·5초 표본 중앙값, peak는 전환 전체다. 빈 화면은 worker 유휴 회수 후와 회수 전을 분리한다.
- idle CPU는 10초 안정화 후 60초 평균, 1개 logical CPU의 100% 기준이다. wakeups·실행 thread·전력(가능 시)·profiler 오버헤드를 기록한다.

## 4. 초기 목표

기준 장비·기본 2~4MP 코믹 corpus·전체 맞춤·한 장/정지 읽기에 적용한다. 두 장·4K·고해상도·solid random jump는 별도 결과와 제한을 공개한다.

| 항목 | 목표 |
|---|---|
| 콜드 파일/ZIP→정상 첫 페이지 | p95 ≤ 1초 |
| RAR→정상 첫 페이지 | 비solid p95 ≤ 1.5초, solid는 크기/방법별 M0 결과로 게이트 확정 |
| 웜 prefetch hit→정상 표시 | p95 ≤ 40ms |
| 일반 폴더/ZIP prefetch miss | p95 ≤ 200ms |
| 주요 입력→선택/모드 상태 응답 | decode 중 p95 ≤ 50ms |
| 빠른 넘김 | 10회/초·마지막 요청만 반영, 입력 종료 후 기본 파일/ZIP에서 p95 ≤ 250ms 안에 최신 정상 page 수렴 |
| GUI dispatch | event 최대 16개 또는 3ms·texture admission 1개/8MiB, 실제 지연 upload 별도 |
| 정지 화면 CPU | 1 logical CPU 기준 평균 ≤ 0.5%, 상시 polling/redraw 없음 |
| 종료 | p95 ≤ 1.5초·worker/handle/permit 정상 잔류 0 |

### 경량성 메모리 목표

| 상황 | 합산 RSS 초기 목표 |
|---|---:|
| 빈 창·worker 회수 후 | 안정 ≤ 100MiB |
| 기본 만화 폴더/ZIP 탐색 | 안정 ≤ 256MiB, 전환 peak ≤ 384MiB |
| 기본 RAR 감상 | 안정 ≤ 256MiB, 비solid peak ≤ 384MiB; solid workspace 별도 측정/승인 |
| 12MP 이상/두 장/4K | 기본값에서 수용/거부·전체 live bytes·RSS/PSS를 별도 공개, 기본 만화 SLA와 혼합하지 않음 |

192MiB는 전체 RSS가 아닌 이미지 관련 예약 한도다. 빈 창/GUI 자체 비용이 목표를 넘으면 사용자에게 제한을 더 낮추기 전에 GTK build/API·불필요한 위젯/폰트/초기화·다른 가벼운 frontend를 비교한다.

- 목표를 위해 정상 기본 corpus의 실패/거부·저품질 고정·오래된 페이지 표시를 허용하지 않는다. 부드러움은 정상 화질·실패율과 함께 판정한다.
- 10,000회 넘김에서 1,000회마다 작업 완료/5초 대기 후 안정값을 수집한다. 첫 2,000회 warm-up 제외 후 초기 3점 대비 마지막 3점 중앙값 증가 **max(8MiB, 초기값의 5%) 이하**를 목표로 하고 지속 상승은 allocator/lease/handle 분석 대상으로 둔다.
- 되풀이가 많은 solid·앞뒤 이동은 bytes read/해제·CPU·worker restart를 기록한다. 메모리 감소가 무제한 반복 해제/CPU 폭증으로 바뀌면 실패다.
- GPU upload 기인 50ms 초과 hitch·copy bytes·temporary peak를 추적한다. 기본 탐색의 해당 hitch 발생률 목표는 1% 미만이다. 4K에서 8MiB 분할로 정상 최종 화질까지 도달하는지 검사한다.
- 아직 미정인 solid별 시간/CPU·소프트웨어 renderer 목표는 M0에서 확정하고 1.0 전 gate 없이 넘기지 않는다.

## 5. 필수 테스트

| 계층 | 검증 |
|---|---|
| core | 자연 정렬/큰 숫자·ID·spread/표지/RTL·분할 반쪽·anchor·예산 산술 |
| 작업/수명 | 최신 선택·invalidate 중 완료·shared cancel·비필수 중단·wake lost-race·마지막 owner까지 permit |
| worker/IPC | 잘못된 길이/stride/ID·mutable 공유 출력·crash/hang/OOM·bounded retry·kill/reap·15초 timeout |
| 형식 | signature·EXIF/CMYK/ICC/alpha·희귀/고비트 입력·TIFF 실제 선택 page·첫 합성 frame만 읽기 |
| 압축 | ZIP64/중복/레거시 이름·RAR4/5/solid·7Z method·scan byte/시간·spool temp·경로 탈출/링크 거부 |
| UX | 키/휠/터치 중복 방지·Tab/Enter/IME·한/두 장·끝 멈춤·500ms 이전 화면 제거·error/취소·Orca |
| 기록 | 표시 완료 기준 저장·book revision 변경·기록 없이 열기·삭제·동시 저장·손상/미래 schema·비UTF-8 round-trip |
| 자원 | 64/192/384MiB·두 패널·zoom/DPI·4K upload·숨김 thumbnail·비가시 취소·worker 유휴 회수·장시간 plateau |
| 배포 | 깨끗한 Linux·GTK/codec/worker 누락·desktop/MIME·수동 업데이트/제거·원본 불변 |
| 드롭·연결 | URI/FileList/GFile·다중/원격 거부·SMB/FTP/SFTP 브리지·늦은 응답/종료·stat 최초 갱신·로그 비식별 |

Rust fmt/Clippy/test와 허용 feature 조합을 CI에 둔다. proptest로 순수 알고리즘, 필요 시 작은 scheduler 모델에 loom, IPC/media에 fuzz, 순수 Rust 수명에 가능한 Miri, native 경계에 지원 sanitizer를 적용한다. 처음부터 모든 도구를 위한 추상 계층을 만들지 않는다. headless GUI는 실제 Linux session/접근성/renderer 시험을 대신하지 않는다.

GUI 회귀는 격리된 Xvfb/Openbox·개별 세션 D-Bus와 합성 입력으로 실행한다. 모의 KIOFuse 서비스는 실제 사용자 세션 버스에서 실행하지 않는다. 실제 FTP 검사는 루프백 익명 읽기 전용 서버와 사용자 세션의 KIOFuse가 필요하며, SMB/SFTP 실제 인증·호스트 키·GVfs 전체 경로·장애 네트워크를 통과한 것으로 계산하지 않는다. 새 검증 도구의 성공은 07의 p95/메모리 성능 게이트가 아니다.

## 6. 개발 순서

| 단계 | 구현/실험 | 통과 산출물 |
|---|---|---|
| M0 PoC | GTK 빈 창·Rust worker 한 장·JPEG 축소/PNG peak·ZIP/RAR·IPC/texture lease·wake/취소 | 실제 환경/버전·bytes/copy/지연/CPU·GUI/worker 수 선택·Linux 보호/라이선스 결과 |
| M1 읽기 수직 경로 | Cargo core/backend/worker/Linux·한 페이지·예산·정렬/선택·취소/종료·계측 | 기본 파일/ZIP·원본 불변·실패/수명 회귀, 내부 검증판 |
| M2 Linux MVP | 모든 P0·RAR/CBR·한/두 장/RTL·확대·opt-in 이어보기·기본 접근성/패키지 | 기본 corpus 실패 0·P0 ID→테스트 연결·192MiB/응답 목표·10,000회 |
| M3 형식/읽기 확장 | P1 형식/7Z/TAR·TIFF·분할·연속·북마크/다음 책/단축키 | 형식별 실제 capability·메모리/solid/scroll 경계·전체 UX |
| M4 Linux 1.0 | 잔여 최적화·접근성·배포/고지/보안 | 모든 P0/P1·확정 목표·설치/업데이트/제거·장시간 gate |
| 이후 | P2를 사용 사례와 비용 기준으로 개별 선택 | 별도 승인/명세/회귀, 자동 범위 확장 없음 |

M0에서도 전체 앱을 먼저 만들지 않는다. thread/worker 추가·GUI 교체·codec fallback·예산 증가에는 재현 사례와 전후 측정이 있어야 한다. 사용자 구현 요청에 따라 M1 수직 경로를 착수했지만 미완료 M0 게이트를 통과로 간주하지 않는다.

## 7. 출시 게이트와 비교

- MVP는 P0, 1.0은 P0+P1의 실제 Linux 패키지 corpus/회귀를 통과한다. 지정 형식 실패·Linux 환경 미확보는 범위 재승인을 받는다.
- 알려진 고위험 메모리 손상·경로 탈출·무제한 자원 사용·원본 변경/사용자 데이터 손상 0건. 보호 실패를 숨기지 않는다.
- 지연/메모리 목표 미달은 원인·전후 측정·대체 정책·승인된 수치 변경을 남긴다. 작은 RAM/기본 corpus 실패로 수치를 맞추지 않는다.
- SBOM/취약점/고지·소스/빌드 안내·개인정보·Linux 설치/제거를 검증한다. 다른 OS의 signature/공증·Android 패키지는 현재 게이트가 아니다.
- Linux 동작 비교는 Gwenview 등 실제 설치 버전/Qt plugin 목록과 같은 파일·창/색 정책·cache 상태·desktop으로 한다. 종합 편집 기능 수가 아닌 첫 표시·페이지 넘김·RSS/PSS·idle CPU를 비교한다.
- 꿀뷰와 Android 앱은 현재 UX 참고다. Wine/emulator 결과로 native Linux와의 속도/메모리 우열을 주장하지 않는다. 해당 OS 실측 없이 “꿀뷰보다 빠름”을 홍보하지 않는다.

## 8. 남은 결정

- 특정 Android “코믹뷰” 앱의 공식 링크·버전·참고할 화면. 미지정 동안 02의 일반 패턴을 사용한다.
- 새 코드 라이선스·앱 ID·Linux 기준 기기/배포 dependencies.
- GTK4만 사용할지, libadwaita가 실제 필요한지, codec별 실제 crate/native 버전과 feature.
- 한 worker prefetch 취소/재시작이 목표를 충족하는지, 필요 시 worker 2개 모드의 순이익.
- solid RAR/7Z·색관리·큰 이미지의 제한과 실제 지원 method 표.

이 항목은 문서 작성만으로 해결된 것으로 표기하지 않는다. 방향을 바꾸면 기준 문서와 관련 테스트/측정 계획도 함께 수정한다.
