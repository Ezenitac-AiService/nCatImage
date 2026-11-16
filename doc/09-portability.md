# 09. Windows/macOS 포팅을 위한 경계

2026-11-16 · v0.5.0. 포팅 경계 분리는 Linux 배포에 포함되지만 Windows/macOS 실행 앱은 아직 미완료다. 기존 0.4.3 배포본과 구분한다.

## 현재 구조

```text
ncat-linux                   Linux 진입점 + KIOFuse/GVfs 인증·마운트
  └─ ncat-gtk                재사용 GTK 화면/입력 어댑터/비동기 작업 수명
       ├─ ncat-presentation  툴킷·OS 독립 읽기 상태/명령/표시 정책
       │    └─ ncat-core     정렬·읽기 방향·spread·크기·예산/lease
       └─ ncat-backend       coordinator·cache·worker IPC·원본 FD 기반 pixels
            ├─ ncat-platform OS 파일 리비전·private 파일·실행 파일 이름
            └─ ncat-protocol DTO·유한 IPC + 별도 native path 모듈

ncat-worker                  IPC dispatch
  ├─ ncat-platform           Linux worker 보호 (실패 시 실행 거부)
  └─ ncat-media              코덱/ZIP·catalog·decode (GUI에는 링크하지 않음)
       └─ ncat-platform      파일 리비전·native 이름·no-follow 출력 열기
```

배포 준비용 로컬 GTK 진입점 `ncat-desktop`을 포함해 현재 10개 crate다. 이 진입점 추가만으로 Windows/macOS의 native 파일/worker 보호가 구현되지는 않는다. [배포 파이프라인](10-distribution.md)은 미구현 OS를 차단한다. 세 가지 의미의 “공통”을 구분한다:

| 계층 | 재사용 범위 | 교체/추가할 부분 |
|---|---|---|
| `ncat-core`, `ncat-presentation` | GTK·OS 의존성 없는 Rust | 새 셸에서 normalized key/command와 `ReaderView` 사용 |
| `ncat-gtk` | GTK를 선택하는 OS의 frontend 기반 | GTK 배포·메뉴·배율·접근성·OS 단축키/대화상자 검증 |
| backend/media/protocol DTO | 재사용 처리 로직; 아직 전체 Windows 빌드 가능은 아님 | 아래 OS 경계를 구현한 뒤 해당 OS에서 통합 검증 |
| `ncat-linux` | Linux 전용 실행/연결 서비스 | 새 실행 crate와 OS 네트워크·인증 어댑터 |
| `ncat-platform/src/unix.rs` | Unix identity·권한·open 구현 | Windows file ID/change time·ACL·reparse-point 방어/공유 모드 |
| `ncat-platform/src/linux.rs` | Linux worker 보호 | macOS/Windows 보호 계약·부모 종료 연동·자원 제한 |
| `ncat-protocol/src/path.rs` | Unix raw bytes (wire v1 표현 유지, 현재 제어 wire v2) | Windows 무손실 native 경로 인코딩·검증·wire 계약 |
| `packaging/linux`, desktop tools | Linux 전용 | 서명·설치·파일 연결·OS별 배포·desktop 시험 |

## GUI 경계

`ncat-presentation`은 위젯이나 backend를 소유하지 않는다. 다음 정책을 제공하며 GTK가 실제로 호출한다:

- `input::command`: 전역 열기/종료 → 명시적 클릭 후 native 입력 → 읽기 명령 우선순위.
- `PageKey`, `page_step`, `adjacent`: RTL 좌우 매핑·방향 독립 이전/다음·페이지 경계.
- `ReaderView`: 확대/축소·맞춤·회전·decode 크기 상한·이미지 표시 크기.
- `interface_shown`: 전체화면 UI 숨김과 일반 창 표시 설정.

GDK 이벤트는 `ncat-gtk/src/input.rs`에서 normalized key로 변환한다. 현재 GTK 셸은 기존 Linux Control 키 규칙을 유지한다. macOS Command 매핑·메뉴·native UX는 향후 어댑터에서 결정/검증해야 한다.

`ncat-gtk`는 widgets·texture/FrameBytes owner·실제 focus·scroll/pan·GLib timer/future·backend 이벤트 수명을 담당한다. 선택/book/로딩·드롭/네트워크 요청 수명 조정도 아직 GTK frontend에 있다. 완전한 headless application controller로 모두 추출했다고 주장하지 않는다. 다른 native 툴킷을 사용하면 이 wiring은 새 셸에서 구현하고 presentation/core/backend를 재사용한다.

네트워크는 `ncat_gtk::run(Resolver)`로 주입한다. `Resolver`는 GIO file과 GTK parent를 받아 future를 반환하는 **GTK 전용 서비스 계약**이다. 일반 Rust/Swift/C# ABI가 아니다. URI 검증/오류 표시/요청 취소·60초 제한은 GTK 쪽, KIOFuse D-Bus·GVfs mount/authentication은 Linux 쪽이다. future를 버려도 OS mount 자체 취소를 보장하지 않는 기존 계약을 유지한다.

## OS·보안 경계

- backend/media의 중복 Unix 리비전을 `ncat-platform::Revision`으로 합쳤다. device/inode/length/mtime/ctime를 모두 유지하며 length/mtime만 비교하는 약한 fallback을 만들지 않는다.
- private 디렉터리 0700·로그 create-new 0600, 기존 출력의 `NOFOLLOW` open은 Unix adapter에 있다. 부모가 기존 출력 FD를 보유하고 worker가 반환한 경로를 다시 열지 않는 구조를 유지한다.
- Linux의 address-space/core rlimit·parent-death SIGKILL·no_new_privs·부모 재확인은 `protect_worker()`로 옮겼다. 다른 OS용 보호는 구현되지 않았다. Linux 이외 보호 호출은 Unsupported이며 worker는 보호 실패 시 종료한다. Windows platform 파일 API 자체도 아직 없다.
- `RawPath`는 별도 `path.rs`에 격리했지만 **Unix byte 형식 그대로**다. lossy UTF-8 변환으로 Windows 포팅을 흉내 내지 않는다. 현재 Windows 전체 workspace 빌드는 불가능하다.
- worker 이름 선택은 OS adapter를 거쳐 `.exe` 이름을 준비했다. 이것만으로 Windows 프로세스 보호·파이프·파일 공유·임시 파일 정리 호환성이 확보된 것은 아니다.
- worker 통합 시험의 가짜 executable·권한·symlink·비UTF-8 경로 fixture는 Unix 전용이다. 다른 OS 시험은 동등한 안전성 사례로 새로 작성해야 한다.

## 후속 포팅 순서

1. GTK 재사용 또는 native frontend 선택. native frontend는 presentation 명령을 해당 툴킷 이벤트로 연결한다.
2. Windows native 경로/wire 인코딩과 file ID/change-time, private ACL, reparse 방어, 열린 임시 파일의 sharing 계약을 구현한다.
3. Windows/macOS worker 보호와 부모 종료·취소/kill/reap을 구현한다. 보호 없는 codec 실행으로 우회하지 않는다.
4. OS 네트워크 인증/마운트, 파일 대화상자·드롭·명령행·앱 lifecycle을 연결한다.
5. OS별 통합/desktop/security 회귀 및 서명·패키징을 수행한다. macOS가 Unix라는 이유로 Linux 보호가 동작한다고 간주하지 않는다.

이번 검증 결과는 [08](08-implementation-status.md)의 미릴리스 절을 따른다. 현재 설치 target은 Linux 하나이며 Windows/macOS 컴파일/실행 검증은 하지 않았다. 공통 두 crate만 대상으로 하는 OS별 CI 정의를 추가하되 실제 CI 결과가 나오기 전 통과했다고 보고하지 않는다.
