# nCatViewer

**가볍게 열고, 빠르게 넘기는 Rust 기반 로컬 코믹뷰어.**

폴더와 ZIP/CBZ를 바로 열어 이미지를 감상하는 Linux용 뷰어입니다. 원본 파일을 변경하지 않으며, 클라우드 연결이나 감상 기록 저장 없이 동작합니다.

> **v0.4.0 · 개발 프리뷰** — 아직 MVP/정식 안정판이 아닙니다. 지원 형식과 아래 제한 사항을 확인해 주세요.

## 다운로드 및 실행

[**Linux x86_64 AppImage 다운로드**](https://github.com/NyarCat/nCatImage/releases/tag/v0.4.0)

GTK와 이미지 처리 worker를 포함한 **AppImage 파일 하나**로 배포합니다. Rust나 GTK 개발 패키지를 따로 설치할 필요가 없습니다.

```bash
chmod +x nCatViewer-0.4.0-x86_64.AppImage
./nCatViewer-0.4.0-x86_64.AppImage
./nCatViewer-0.4.0-x86_64.AppImage /path/to/book.cbz
```

- 대상: **Linux x86_64, glibc 2.39 이상**(Ubuntu 24.04 및 동급 이상 환경).
- X11 또는 Wayland 그래픽 세션과 시스템 그래픽·폰트 환경이 필요합니다. Windows/macOS, ARM64, musl 기반 Alpine은 지원 대상이 아닙니다.
- 호환성을 위해 배포본은 기본적으로 Cairo 소프트웨어 렌더러를 사용합니다. `GSK_RENDERER`를 지정하면 변경할 수 있습니다.
- 모든 배포판·GPU·입력기 조합에서 검증된 것은 아닙니다. AppImage가 모든 Linux에서 실행된다는 뜻은 아닙니다.
- 외부 fcitx/IBus 모듈은 포함하지 않습니다. 시스템 입력기 설정에 따라 경고나 한글 입력 문제가 있을 수 있으며 별도 검증이 필요합니다.

FUSE 관련 오류가 나면 설치 없이 압축 해제 실행을 시도하세요:

```bash
./nCatViewer-0.4.0-x86_64.AppImage --appimage-extract-and-run
```

다운로드 파일의 무결성은 릴리스에 함께 제공하는 `SHA256SUMS`로 확인할 수 있습니다:

```bash
sha256sum -c SHA256SUMS
```

## 주요 기능

- 이미지 파일, 폴더, ZIP/CBZ 열기와 파일 드래그 앤 드롭
- 자연 정렬, 이전/다음 페이지 및 페이지 번호 이동
- JPEG, PNG, BMP, WebP, GIF 첫 프레임 표시
- EXIF 방향 적용, 임시 회전, 맞춤, 확대, 스크롤·드래그 패닝, 전체 화면
- 좌우 읽기 방향 전환
- 별도 worker 프로세스에서 이미지 처리, 오래된 요청 취소
- 바이트 제한 페이지 캐시와 인접 페이지 미리 읽기

### 현재 제한

RAR/CBR·7Z, 두 장 보기 UI, 이어보기, 썸네일, ICC 색관리, 애니메이션 재생은 아직 지원하지 않습니다. 폴더는 하위 폴더를 탐색하지 않습니다.

현재 출력은 **최대 8MiB 미리보기**이며 원본 해상도·4K 최종 화질을 보장하지 않습니다. 큰 이미지는 메모리 제한으로 거부될 수 있습니다. 기본 이미지 처리 예약 예산은 GUI와 worker 합산 192MiB이며, 앱 전체 RSS 제한과는 다릅니다.

## 단축키

| 동작 | 키 |
|---|---|
| 파일 / 폴더 열기 | `Ctrl+O` / `Ctrl+Shift+O` |
| 다음 / 이전 페이지 | `Space`, `PageDown` / `Backspace`, `PageUp` |
| 읽기 방향에 따른 페이지 이동 | `←` / `→` |
| 읽기 방향 전환 | `D` |
| 확대 / 축소 / 맞춤 | `+` / `-` / `9` |
| 회전 / 반대 회전 | `R` / `Shift+R` |
| 다시 열기 | `F5` |
| 전체 화면 / 종료 | `F11` / `Ctrl+Q` |

입력 위젯에 포커스가 있으면 해당 위젯의 기본 키 동작을 우선합니다.

## 소스 빌드

저장소의 `rust-toolchain.toml`에 지정된 Rust와 GTK **4.10 이상** 개발 패키지가 필요합니다. Ubuntu 24.04에서는 다음과 같이 빌드합니다:

```bash
sudo apt install build-essential pkg-config libgtk-4-dev
cargo build --workspace --release --locked
./target/release/ncat-linux /path/to/book.cbz
```

`ncat-linux`와 `ncat-worker`는 같은 디렉터리에 두세요. GUI만 빌드하면 worker가 누락될 수 있으므로 workspace 전체를 빌드해야 합니다.

AppImage 생성 방법과 필요한 추가 패키지는 [`packaging/linux/build-appimage.sh`](packaging/linux/build-appimage.sh)에 있습니다. 배포용 빌드는 최신 개발 머신이 아닌 Ubuntu 24.04 기준 환경에서 만드세요.

## 실행 설정 및 오류 보고

| 환경변수 | 기본값 | 설명 |
|---|---:|---|
| `NCAT_MEMORY_MIB` | 192 | 이미지 관련 예약 예산, 1~384MiB |
| `NCAT_CACHE_MIB` | 총 예산의 1/8 | 캐시 상한, 0이면 비활성화, 최대 총 예산의 1/3 |
| `NCAT_WORKSPACE_MIB` | 128 | 작업 공간 상한, 8~384MiB, 총 예산으로 추가 제한 |
| `NCAT_TIMEOUT_SECS` | 15 | 작업 제한 시간, 1~120초 |
| `NCAT_PREFETCH` | 1 | 인접 페이지 미리 읽기, 0/1 또는 false/true |

일부 worker 오류가 발생하면 **실행한 현재 디렉터리의 `log/`**에 진단 로그를 저장합니다. 원본 경로·파일명·이미지 내용은 기록하거나 외부 전송하지 않습니다. 앱 로그는 최근 최대 8개를 보관하며 직접 삭제할 수 있습니다.

[이슈 등록](https://github.com/NyarCat/nCatImage/issues) 시 배포판, X11/Wayland, 앱 버전, 재현 방법과 관련 로그를 첨부해 주세요. 개인 이미지나 인증 정보는 올리지 마세요.

## 개발 상태

Linux x86_64부터 개발 중인 개인 프로젝트입니다. 정식 코덱 호환성·보안·성능 검증이 완료된 제품이 아니며, 다른 뷰어보다 가볍다는 비교 결과를 주장하지 않습니다. 코드 라이선스는 아직 미정입니다. 배포본에 포함된 외부 구성요소의 라이선스 고지는 AppImage 내부 `usr/share/doc/ncatviewer/`에 있습니다.
