# nCatImage 작업 내역

이 문서는 실제 반영한 작업을 기록한다. 계획·목표와 검증 범위는 [개발 문서](doc/README.md), 현재 제한은 [구현 상태](doc/08-implementation-status.md)를 따른다. OpenCode 코딩 에이전트를 통한 AI 기반 개발이며, AI 생성·수정 코드와 문서가 포함된다.

## 0.5.2 — 2026-11-17

- **Windows x86_64 실험판 포함: 실제 Windows에서 테스트하지 않았다.** 정상적인 이미지 표시·보안·입력 동작을 보장하지 않으며 Wine에서는 Job 제한 검증 실패로 protected decode가 차단된다. 정식 readiness는 false다.
- [개발 계획](doc/11-windows-single-exe-wine.md), GTK 비의존 Win32 뷰어·단일 EXE PE import 검사·private Wine prefix 도구와 개발 CI를 추가했다.
- Windows UTF-16LE 경로·invalid wire 경로 거부, 공통 worker executable/argument 경계, 동일 EXE 재실행을 위한 worker library entry를 준비했다.
- Windows file ID/private DACL/final reparse 검사·strict Job handshake·같은 EXE worker를 구현하고 탐색/보기/애니메이션 UI를 연결했다. native 보안·부모 crash·DPI/접근성·FTP/SFTP는 미검증/미완료다.
- Linux Rust 90개(장기 1개 ignored), GTK 8개, Python 7개, Windows-target Wine unit 30개(native symlink 1개 ignored) 및 platform/빈 창/worker 거부 3개 통과. Windows native 검증과 구분한다.

## 0.5.1 — 2026-11-16

- 디코딩 busy/프레임 경계마다 이전·다음 버튼을 비활성화하던 처리를 제거했다. 자동 재생 중 page navigation/title 갱신도 반복하지 않는다.
- 프레임/전체 수·상태·배속 표시를 타이틀바에서 애니메이션 툴바의 동일 행으로 옮겼다.
- 일시정지 중 1-based 프레임 번호·이동 버튼/Enter를 추가했다. 아직 전체 수를 모르는 입력의 범위 초과는 재생 wrap/프로토콜 오류 대신 마지막 유효 프레임으로 clamp한다.
- wire v3의 bounded total metadata와 프레임 이동/버튼 상태 회귀를 추가했다. GUI/worker는 같은 빌드로 배포한다.
- Rust 테스트 82개(장기 1개 ignored), 실제 GTK 재생/컨트롤 검사 8개·fmt·Clippy·release 빌드 통과. AppImage는 CI의 패키지 검사를 통과한 artifact로 제공하며 Windows/macOS 차단은 유지한다.

## 0.5.0 — 2026-11-16

Linux x86_64 AppImage 우선 배포. Windows/macOS는 native 파일/worker 보호 미구현으로 차단되며 ZIP/DMG 배포본이 없다. 기존 v0.4.3 Release/태그는 유지한다.

### 애니메이션 재생

- 타이틀바의 현재/전체 프레임·상태·배속 표시, 이전(`,`)/다음(`.`)/재생·일시정지(`/`) 버튼·단축키와 0.25×~4× 속도 선택을 추가했다. 전체 프레임 수는 첫 주기 완료 뒤 확인한다.
- 이전 프레임은 bounded worker seek로 다시 합성한다. finite 재생 종료 뒤에도 예약된 세션을 유지해 이전 이동/재시작을 지원하고 새 페이지·취소·종료에서 해제한다. 모든 decoded frame을 저장하지 않는다.

- GIF/AGIF·WebP·APNG의 합성 프레임 순차 재생, 지연·유한/무한 반복을 연결했다. `.png` APNG와 ZIP/CBZ 내부 입력도 같은 worker 경로를 사용한다.
- persistent decoder/compositing workspace를 parent 예산에서 재생 수명 동안 예약하고, 애니메이션의 정지 cache 삽입·인접 prefetch를 억제한다. 이동/보기 변경·취소/종료는 이전 session을 해제한다.
- wire v2에 다음 프레임/재생 종료·유한 timing metadata를 추가했다. GUI와 worker는 같은 소스 버전으로 함께 빌드해야 한다.
- 합성 GIF/APNG disposal-previous, WebP 부분 프레임/background disposal, 반복·지연·예산/lease·원본 변경·ZIP/회전 회귀와 실제 GTK 7개 입력 재생 smoke를 추가했다. 전체 실물 형식·정확한 compositor FPS·일시정지/배속 UI 인증은 아니다.

- OS별 Distribution packages workflow를 추가했다: Linux AppImage, Windows GTK/worker 동봉 포터블 ZIP, macOS arm64 `.app`/DMG. Windows 단일 EXE는 구현하지 않고 ZIP fallback을 선택했다.
- native 포팅 readiness·태그/버전 검사·패키지 relocation smoke·체크섬·전 OS 성공 후 초안 릴리스 정책을 추가했다. 현재 Windows/macOS는 미구현 보호/파일 API 때문에 차단된다.
- `ncat-desktop` 로컬 GTK 진입점과 배포 문서를 추가했다. 기존 CI의 Openbox 시작 경합은 WM 준비를 기다리도록 보완했다.

- `ncat-presentation`에 툴킷 독립 읽기 명령·키 우선순위·페이지 이동·확대/회전·보기 크기·UI 표시 정책을 추출했다.
- GTK frontend를 `ncat-gtk`로 옮기고 Linux 진입점은 네트워크 연결 서비스를 주입한다. KIOFuse/GVfs 구현은 `ncat-linux`에 남긴다.
- 중복 파일 리비전과 private 파일·no-follow 출력·Linux worker 보호를 `ncat-platform`으로 분리했다. Unix 경로 인코딩은 protocol의 별도 모듈에 격리했다.
- 공통 로직 OS별 CI 정의와 경계 테스트, [포팅 문서](doc/09-portability.md)를 추가했다. Windows/macOS 앱 구현/실행 검증 완료는 아니다.

## 0.4.3 — 2026-11-16

### 네트워크 폴더

- `smb://`, `ftp://`, `sftp://`를 OS 연결 도구의 로컬 경로로 해석한다. KIOFuse D-Bus 연동을 우선하고, 사용 불가 시 GIO 원격 백엔드와 GVfs/FUSE를 시도한다.
- 네트워크 버튼·Ctrl+L 주소 입력, 드롭, 파일/폴더 대화상자, 명령행을 공통 열기 경로로 연결했다.
- 새 열기·페이지 선택·종료 시 이전 연결 future를 취소하고 요청 번호로 늦은 응답을 무시한다. 앱 결과 대기는 최대 60초이며 OS 마운트 자체의 수명은 OS 도구 정책을 따른다.
- 프로토콜·URI 형식·호스트·16KiB 길이·응답 경로를 검사한다. 비밀번호 포함 URI는 거부한다. 계정 관리·영구 URI 저장·자체 다운로드 캐시는 추가하지 않았다.
- 실제 FTP 폴더/CBZ의 500회 탐색을 확인했다. SMB/SFTP는 모의 연결 도구로 요청 전달·표시·취소를 확인했으며 실제 서버 인증·GNOME/GVfs 전체 경로는 별도 미검증이다.

### 드롭 및 진단

- `GdkFileList`/URI 목록을 우선하고 단일 GFile도 받는다. 한 항목만 허용하며 자식 입력 위젯보다 먼저 드롭을 처리한다.
- X11 표준 URI 드롭은 기존 빌드에서도 성공했다. 사용자 Wayland 로그에서는 네트워크 항목이 수신된 뒤 `NonLocal`로 거부되었고, 포털이 로컬 경로를 준 드롭은 표시에 성공한 것을 확인했다. 모든 드롭 실패가 형식 문제라고 단정하지 않는다.
- 시작 준비, 진입, 형식 협상, 항목 수, 거부, 열기, 표시, 네트워크 연결·취소·실패를 정제된 타입으로 기록한다.
- GUI 진단 큐는 최대 32개이며 기존 coordinator가 디스크에 쓴다. 직전 128개 상태, 줄당 최대 1KiB, 최근 관리 로그 최대 8개, 파일 권한 0600을 유지한다. 경로·파일명·URI·서버·자격증명은 기록하지 않는다.
- FUSE 첫 읽기 중 stat 속성 갱신으로 발생한 Changed를 실제 FTP에서 재현했다. catalog/frame 게시 전 최초 열기를 새 worker로 한 번 재시도하며 기존 리비전 검사는 제거하지 않았다.

### 문서·검증·배포

- 모든 개발 Markdown, worker 통합 테스트, backend 예제, 합성 입력·메모리·GUI·네트워크 검증 도구와 Linux CI를 공개한다. 사용자 ZIP·개인 이미지·로그·실행 바이너리는 소스 커밋에서 제외한다.
- README에 AI 코딩 에이전트 기반 개발 사실, 변경 내역, 사용법·보안·미검증 범위를 명시했다.
- Linux x86_64 AppImage를 Ubuntu 24.04 기준으로 빌드한다. GTK와 worker를 포함하며 glibc 2.39 이상, 기본 Cairo renderer, 체크섬·고지를 제공한다. 개발 프리뷰이며 코드 서명·완전 static·모든 배포판 지원은 보장하지 않는다.
- 기본 Rust 테스트 56개 및 수동 장기 시험 1개 ignored. 포맷·Clippy·회귀 실행 결과와 재현 명령은 08 및 tools 문서에 기록한다.

## 0.4.2 — 2026-11-15

- 앱 이름을 저장소와 같은 nCatImage로 통일했다. 창 제목·오류 메시지·앱 ID(`io.github.ncatimage.Development`)·데스크톱/아이콘·AppImage 파일명·내부 고지 경로를 변경했다.
- README 사용법과 Linux AppImage 배포를 갱신했다. 당시 드롭·네트워크 보강은 아직 릴리스에 포함되지 않았다.

## 0.4.1 — 2026-11-15

- 페이지 정보와 조작부를 하단에서 상단으로 이동했다.
- 일반 창에서 백틱으로 상단 전체를 토글하고 전체화면에서는 숨긴다. F11 복귀 시 이전 표시 설정을 복원한다.
- 상단 UI를 클릭해야 컨트롤 키보드 조작을 활성화하며 그 전 Tab 포커스 진입을 막는다. 이미지 클릭 후 읽기 단축키로 돌아간다.
- ↑/↓를 이전/다음 페이지로 매핑하고 메인 뷰어 Esc를 종료로 변경했다. 별도 대화상자의 Esc는 취소다.
- 키보드·실제 마우스 입력·전체화면 회귀와 AppImage를 확인했다.

## 0.4.0 — 2026-11-15

- Rust 6개 crate workspace, Linux GTK 단일 페이지 UI, 별도 worker의 파일/폴더/ZIP/CBZ 열기, 자연 정렬·기본 정지 이미지·맞춤/확대/회전/패닝을 연결했다.
- 192MiB 예약 예산·마지막 owner까지 유지되는 픽셀 lease, byte 제한 LRU·인접 페이지 prefetch·세대별 결과 무효화·유한 worker 복구와 진단을 구현했다.
- 사용자 대용량 ZIP의 빠른 이동 오류를 조사했으나 원래 오류의 정확한 원인을 재현·확정하지 못했다. 변경되지 않은 소스의 모순된 worker 응답에 한해 복구를 보강했다.
- README·패키징 도구와 첫 Linux AppImage 개발 프리뷰를 배포했다. JPEG 축소 디코딩·ICC·RAR·두 장 UI·이어보기·정식 성능/접근성 게이트는 미완료다.
