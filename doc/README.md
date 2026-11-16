# nCatImage 개발 문서 지도

갱신: 2026-11-16 · v0.5.1 · 상태: **Linux M0 일부 실험·M1 개발 프리뷰 / MVP 미완료**

OpenCode 코딩 에이전트를 통한 AI 기반 개발이다. 사용자용 안내는 [README](../README.md), 버전별 전체 작업 내역은 [CHANGELOG](../CHANGELOG.md), 재현 가능한 검증 코드는 [tools 안내](../tools/README.md)를 따른다. AI 생성·수정 내용과 자동 검사 통과를 정식 품질·보안 인증으로 취급하지 않는다.

## 제품 정의

**최대한 가벼우면서 다양한 이미지와 압축파일을 빠르고 편하게 읽는 Rust 코믹뷰어.**

우선순위는 **읽기 흐름 → 입력 반응성 → 메모리 절약 → 형식 확장**이다. 형식을 늘리더라도 기본 감상 경로의 runtime·캐시·시작 비용을 불필요하게 늘리지 않는다. 안전 검증은 성능을 이유로 생략하지 않는다.

## 문서 구성과 기준

| 문서 | 결정하는 내용 |
|---|---|
| [01 제품](01-product.md) | 목표·기능 ID·MVP/1.0·명시적 제외 |
| [02 읽기 UX](02-reader-ux.md) | 화면·키/마우스/터치·한/두 장·읽기 방향·이어보기 |
| [03 구조](03-architecture.md) | Rust 모듈·GUI 경계·worker·취소·소유권 |
| [04 형식/압축](04-formats-archives.md) | 실제 지원 범위·backend 후보·색/페이지·solid 정책 |
| [05 자원](05-memory-performance.md) | 바이트 예산·피크·캐시·prefetch·줌/업로드 |
| [06 보안/데이터](06-security-data.md) | 입력 제한·worker 보호·설정/기록·라이선스/패키지 |
| [07 검증/로드맵](07-validation-roadmap.md) | 측정·합격 기준·Linux 단계·미결정 사항 |
| [08 구현 상태](08-implementation-status.md) | 실제 코드/실행 결과·기능 ID별 진척·미검증/제한 |
| [09 포팅 경계](09-portability.md) | 공통 presentation·GTK·OS 서비스 및 Windows/macOS 미구현 목록 |
| [10 배포 파이프라인](10-distribution.md) | AppImage·Windows 단일 EXE 전환·macOS DMG, readiness gate·서명/릴리스 정책 |
| [11 Windows 단일 EXE·Wine](11-windows-single-exe-wine.md) | native GUI·동일 EXE worker·Windows 안전성·Wine 단계별 개발 계획 |
| [작업 내역](../CHANGELOG.md) | 0.4.0~0.4.3 반영 사항·릴리스 경계 |
| [검증 도구](../tools/README.md) | 합성 입력·worker·GUI·네트워크 재현 절차 |
| [개발 지침](../AGENT.md) | 에이전트 필수 원칙 |

충돌 시 위 표의 담당 문서를 기준으로 수정하고 다른 문서/테스트 참조도 함께 갱신한다. 자원 수치의 단일 기준은 05, 측정/판정의 단일 기준은 07이다.

## 새 설계의 경계

- 이전 개발 문서는 사용자 요청으로 전부 폐기했다. WPF 이관·편집 필터·종합 사진 기능·다중 OS 동시 개발을 전제로 하지 않는다.
- 실행 검증은 Linux x64다. 사용자 요청에 따라 Windows/macOS 포팅을 준비하는 공통 presentation·GTK frontend·OS 경계를 분리했다. 실제 OS 앱·FFI·배포는 아직 없다. [09](09-portability.md)를 따른다.
- Linux GUI는 gtk-rs/GTK4 우선 PoC다. native 위젯/IME/접근성 활용이 이유이며 가장 가볍다는 검증 결과는 아니다. 더 무거운 GUI 계층은 실측 필요 시만 도입한다.
- 앱/backend 코드는 Rust, 확장 형식의 native 라이브러리 의존성은 허용한다. 무조건 순수 Rust만 고집해 이미지/압축 호환성을 축소하지 않는다.
- 0.4.3은 상단 UI/수동 토글·클릭 후 키보드 입력·드롭 진단과 OS 경유 SMB/FTP/SFTP를 포함한다. 네트워크 URI는 GUI의 Linux 어댑터가 비동기로 연결하고 디코딩은 기존 worker 경계를 유지한다. 자체 계정/다운로드 관리자는 만들지 않는다.
- 최대한 가볍다는 것은 최소 기능·짧은 cache·메모리 압력 대응·낮은 유휴 CPU·불필요한 decode/copy 회피로 검증한다. 임의 RAM 숫자만 작게 설정하고 정상 만화를 못 읽는 것은 성공이 아니다.

## 단계 용어

- **PoC:** 기술 선택/제한/비용을 확인하는 실험. 제품 완성이 아니다.
- **MVP/P0:** 폴더/주요 압축·기본 정지 이미지·한/두 장·읽기 방향·빠른 탐색·기록 opt-in을 갖춘 Linux 코믹뷰어.
- **1.0/P1:** 확장 형식·7Z/TAR·펼침 분할/연속 보기·감상 편의 및 전체 출시 게이트를 충족한 Linux 제품.
- **P2:** 1.0 이후 검토. 필요가 불명확한 기능은 추가하지 않는다.
- **계획/목표:** 미구현/미측정 상태. **지원:** 실제 배포 아티팩트의 corpus·오류 경계를 통과한 조합만 해당한다.

## UI 참고와 확인 범위

- Windows [꿀뷰 공식 자료](https://www.bandisoft.com/honeyview/)의 간결한 감상·압축 내부 보기·키보드/휠 중심 흐름을 참고한다. 공식 기준 버전은 5.53이며 기능/보안 업데이트가 종료되었다. 반디뷰 기능을 꿀뷰 기능으로 섞지 않는다.
- Android “코믹뷰”는 특정 앱/버전/링크가 미확정이다. 화면 숨김·탭/스와이프·읽기 방향·이어보기 같은 **일반 코믹 리더 패턴을 제안**한 것이며 특정 앱의 확인된 기능이라고 주장하지 않는다.
- 참고 앱의 코드·이미지·아이콘·고유 그래픽을 복제하지 않는다. 기능/흐름 참고와 성능 비교·저작물 재사용은 구분한다.
- Cargo workspace·GTK 개발판·worker/통합 테스트·Ubuntu 24.04 기준 AppImage를 추가했다. Xvfb/Openbox X11과 KDE Wayland의 제한된 실행은 확인했으나 실제 Ubuntu GNOME·접근성·정식 corpus/벤치마크 게이트는 아직 통과하지 않았다. SMB/SFTP 실서버 인증 및 GVfs 전체 경로도 미검증이다.
