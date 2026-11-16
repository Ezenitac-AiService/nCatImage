# 10. OS별 빌드·패키징·릴리스 파이프라인

2026-11-16 · 소스 버전 0.4.3 유지. 기존 v0.4.3 Release/태그/다운로드는 덮어쓰지 않는다.

## 형식과 현재 상태

| OS | 계획 아티팩트 | 구성 | 현재 상태 |
|---|---|---|---|
| Linux x86_64 | `nCatImage-VERSION-x86_64.AppImage` | GUI·worker·GTK·리소스 | 구현됨; Ubuntu 24.04/glibc 2.39 기준 재빌드 |
| Windows x86_64 | `nCatImage-VERSION-windows-x86_64-portable.zip` | GUI EXE·worker EXE·GTK DLL·schemas/icons·실행 CMD | 패키징 코드/CI 정의 있음; native 포팅 미완료로 차단 |
| macOS Apple Silicon | `nCatImage-VERSION-macos-arm64.dmg` | `.app`·worker·GTK dylib·리소스·Applications 링크 | 패키징 코드/CI 정의 있음; worker 보호 미완료로 차단 |

Windows는 현재 GTK와 별도 worker 구조상 EXE 하나를 복사해 실행할 수 없다. 단일 파일 self-extracting launcher를 추가하지 않고 사용자가 허용한 **포터블 ZIP fallback**을 선택했다. 압축 해제 후 `nCatImage.cmd`로 실행하며 설치·관리자 권한을 요구하지 않는 구성을 목표로 한다. 실제 Windows 검증 전 “포터블 동작 확인”으로 표시하지 않는다.

macOS는 arm64 native 빌드만 목표다. Intel/Universal binary가 아니다. 최소 OS 버전·배포 환경은 실제 포팅/실행 검사 후 확정한다. 현재 서명은 **ad-hoc preview** 설계일 뿐 Developer ID 서명·Apple 공증이 아니다. Windows Authenticode도 미구성이다. 일반 사용자 배포에는 별도 서명/공증·라이선스 검토가 필요하다.

## 실행

`.github/workflows/release.yml`의 **Distribution packages**:

```bash
# 현재 브랜치 소스로 3개 OS 작업 실행 (공개 Release를 만들지 않음)
gh workflow run release.yml --ref main -f platform=all

# Linux만 패키징 검증할 때
gh workflow run release.yml --ref main -f platform=linux
```

수동 빌드의 성공 산출물은 Actions artifact에서 받는다. Windows/macOS는 현재 `packaging/preflight.py`에서 **명시적 실패**하며 가짜 ZIP/DMG나 보호 없는 worker를 업로드하지 않는다. 서로 독립된 작업이므로 Linux 성공 artifact는 다른 OS 실패와 별개로 보관한다.

향후 새 `vVERSION` 태그를 push하면 3개 OS 빌드를 실행한다. Cargo 버전과 태그가 다르면 실패한다. 세 작업이 **모두** 성공해야 각 runner의 체크섬을 검증하고 통합 `SHA256SUMS`와 함께 **draft + prerelease**를 새로 생성한다. 기존 Release는 수정하지 않는다. 초안의 검증/서명 상태와 변경 내역을 검토한 뒤 별도로 게시한다. 이번 요청에서는 새 태그나 Release를 만들지 않는다.

`contents: write`는 publish 작업에만 부여하며 package 작업은 read-only다. 공개 runner에서는 서명 키가 필요하지 않으며 저장소에 키/인증서를 넣지 않는다.

## 안전한 승격 절차

`packaging/targets.json`은 구현/검증 준비 상태다. `ready: false`를 단순히 true로 바꾸면 포팅이 완료되는 것이 아니다:

1. [09](09-portability.md)의 native 경로·파일 ID/ACL/reparse·worker 보호·부모 종료 기능을 구현한다.
2. native 파일/IPC/security 회귀를 추가한다. `ncat-desktop`은 로컬용 GTK 진입점만 제공하며 Linux 전용 네트워크를 사용하지 않는다.
3. native 빌드와 패키징을 해당 OS에서 검증한다. GTK loader/dylib/DLL·schema/icon·폰트·worker 누락을 검사한다.
4. 압축 해제/읽기 전용 DMG의 다른 위치에서, 개발용 PATH 없이 합성 CBZ를 열어 **패키지 GUI와 동봉된 worker**를 검사한다. `NCAT_WORKER` override로 누락을 숨기지 않는다.
5. 실제 desktop·드롭·IME·접근성·GPU·종료/원본 불변·서명/배포 정책을 검토한 뒤 blocker 목록을 갱신하고 readiness를 승격한다.

Windows DLL closure는 `objdump`로 전이 import를 수집한다. macOS는 `dylibbundler`로 Homebrew dylib 경로를 bundle 내부로 옮기고 `otool`로 시스템 외 절대경로 잔류를 거부한다. 리소스·의존성 고지를 포함하지만 자동 수집은 LGPL 등 법적 배포 의무 검토를 대신하지 않는다.

## 검증 도구

```bash
python3 -m unittest discover -s packaging -p test_pipeline.py
python3 packaging/preflight.py linux
python3 packaging/checksums.py dist
```

Python 테스트는 OS gate·태그 일치·불완전 릴리스 거부·체크섬만 검증한다. Windows/macOS 패키지 실행 성공을 뜻하지 않는다. Linux AppImage 빌드는 기존 upstream 도구의 SHA256 pin을 유지하며 pin이 바뀌면 실패한다. 자동으로 검증을 끄거나 새 hash를 신뢰하지 않는다.

원격 Actions 승인/사용량/runner 정책 또는 네트워크 제한으로 빌드가 차단되면 해당 상태를 그대로 보고한다. 이 문서의 파이프라인 정의를 세 OS 제품 배포 완료로 해석하지 않는다.
