# 검증 도구 — v0.5.1

2026-11-16 기준. 이 도구들은 AI 코딩 에이전트 개발 중 사용한 검사를 재현 가능하게 정리한 코드다. 작은 합성 입력의 회귀 검사이며 실제 책 전체·GPU·IME·접근성·보안·성능 인증이 아니다. 사용자 ZIP·개인 이미지·실제 서버 계정은 저장소에 포함하지 않는다.

## Rust 회귀 및 예제

Windows 단일 EXE/Wine 검사는 [11](../doc/11-windows-single-exe-wine.md)을 따른다. `packaging/windows/build-single.py`는 GTK 비의존 개발 뷰어와 PE import report를 만든다. `tools/windows-wine-smoke.py --image target/windows-single/nCatImage.exe`는 새 private prefix에서 `--self-test-platform`·빈 창·보호 없는 worker 거부를 검사한다. Wine Job readback 제약 때문에 protected decode 성공 검사와 구분한다.

native Windows에서는 `python tools/windows-native-smoke.py --image target/release/ncat-windows.exe`로 보호 handshake·PNG/CBZ pipeline·GUI 표시/재생/500회 탐색을 검사하도록 CI를 정의했다. 이 native 검사는 로컬에서 실행하지 못했으며 통과로 계산하지 않는다. 생성 fixture의 전후 hash·단일 EXE 배포 폴더를 확인하며 NTFS 공격·부모 crash·메모리 고갈·실제 입력/DPI 인증을 대신하지 않는다.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
```

배포된 0.4.3은 기본 테스트 56개, 0.5.0은 80개이며, 0.5.1은 82개다. 수동 10,000회 수명 시험 1개는 ignored다. 툴킷·OS 독립 부분만 검사하려면 `cargo test -p ncat-core -p ncat-presentation --locked`를 사용한다. 수동 시험은 명시적으로 실행한다:

```bash
cargo test -p ncat-worker --test pipeline ten_thousand_decodes_return_all_leases --locked -- --ignored
```

`crates/ncat-worker/tests/pipeline.rs`는 합성 이미지/ZIP과 가짜 worker를 자체 생성한다. IPC 오류·timeout·worker crash·원본 변경·복구 한도·자연 정렬·EXIF·회전·캐시/prefetch·예산/lease를 검사한다. backend의 `inspect`, `cache_bench`, `navigation_stress` 예제는 수동 계측용이며 GUI 제시 지연을 대신하지 않는다. 각 예제의 인자 검사를 먼저 확인하고 공개 가능한 입력만 사용한다.

## 합성 입력과 메모리 smoke

```bash
python3 tools/smoke-fixtures.py /tmp/opencode/my-new-fixtures
python3 tools/measure-smoke.py -- target/release/ncat-linux --smoke-test /tmp/opencode/my-new-fixtures/sample.cbz
python3 tools/measure-smoke.py -- target/release/ncat-linux --smoke-navigation /tmp/opencode/my-new-fixtures/sample.cbz
```

생성 대상은 아직 없는 디렉터리여야 한다. `/tmp/opencode`가 없는 머신에서는 사용자 소유 임시 경로를 사용한다. `measure-smoke.py`의 RSS/PSS 샘플 peak·전체 실행 시간은 compositor 제시 지연·정확한 순간 peak·정식 p95가 아니다.

## 격리된 GUI·드롭·모의 네트워크 회귀

### 애니메이션 회귀

`crates/ncat-worker/tests/animation.rs`는 GIF/APNG와 RIFF WebP 합성 fixture를 자체 생성한다. finite/infinite loop·delay·APNG 별도 default 이미지·부분 프레임/disposal·예산·원본 변경·ZIP·회전·취소/lease를 검사한다. 전 프레임 collect 없이 순차 요청하며 무한 재생 100회 뒤에도 parent 예약이 증가하지 않는지 확인한다.

실제 GTK frame 표시 수명 검사는 Pillow(WebP encoder 포함)가 필요하다. Ubuntu는 `python3-pil`을 설치한다:

```bash
GSK_RENDERER=cairo python3 tools/animation-smoke.py --image target/release/ncat-linux
# 또는 전용 Xvfb:
GDK_BACKEND=x11 GSK_RENDERER=cairo xvfb-run -a dbus-run-session -- \
  /usr/bin/python3 tools/animation-smoke.py --image target/release/ncat-linux
```

명시적 `--smoke-animation` 모드는 animation 입력의 실제 texture 설정/one-shot timer/다음 frame 요청을 최대 6프레임까지 확인하고 종료한다. finite 파일은 최소 2프레임 후 정상 종료도 허용한다. 소스 SHA256 불변도 확인하며 실측 compositor 제시 시간/FPS·실물 codec corpus 인증은 아니다. 일반 `--smoke-test`는 기존처럼 첫 프레임만 표시하고 종료한다.

도구는 `--smoke-animation-controls`도 실행해 실제 GTK 버튼 signal과 속도 dropdown을 조작한다. pause 유지·다음/이전 seek·2× 속도·finite 종료 후 재시작·번호 이동·frame label/버튼의 같은 행 배치와 버튼 sensitivity 전환 0회를 검증한다. OS 키 입력 주입 시험은 아니며 `, . /` 정규화와 modifier 정책은 Rust 단위 시험으로 확인한다. v0.5.1 Rust 기본 테스트는 82개다.

Ubuntu 24.04 예시 의존성:

```bash
sudo apt install xvfb xauth x11-utils dbus-x11 openbox xdotool python3-gi python3-pyatspi at-spi2-core fonts-dejavu-core
```

실제 사용자 세션 D-Bus에 가짜 KIOFuse 서비스를 등록하면 안 된다. **반드시 새 Xvfb와 `dbus-run-session`에서 실행**한다. 도구는 명시적 격리 플래그와 X11을 요구하고 이미 KIOFuse 이름이 점유되어 있으면 중단한다.

```bash
GDK_BACKEND=x11 GTK_A11Y=atspi GTK_THEME=Adwaita GDK_SCALE=1 \
  xvfb-run -a -s '-screen 0 1280x1024x24' dbus-run-session -- \
  env NCAT_PRIVATE_TEST_BUS=1 python3 tools/desktop-regression.py --image target/release/ncat-linux
```

`--image`에는 AppImage 경로도 지정할 수 있으며 FUSE 없이 압축 해제 실행한다. UI 좌표 검사는 Ubuntu GTK/Adwaita·1000×800 창 기준으로 고정되어 다른 테마/배율에서 실패할 수 있다. 입력은 전용 가상 화면에만 전송한다.

확인 항목:

- ↑/↓·좌우 RTL·기본 페이지 키·클릭 전 Tab 차단·클릭 후 native spin 입력·이미지 클릭 복귀
- 상단 UI 토글·전체화면 숨김/복원·입력 중/전체화면 Esc 종료
- 실제 마우스 드래그의 URI/FileList·폴더·다중 항목/미지원 형식·손상 입력 처리
- 모의 SMB/FTP/SFTP 브리지, 네트워크 드롭, Ctrl+L 입력, 늦은 응답 무시·대기 중 종료
- 로그의 경로·파일명·서버·URI·계정 비노출

모의 브리지는 로컬 합성 CBZ를 반환하므로 **실제 SMB/SFTP 인증·네트워크 전송을 검증하지 않는다.** 결과/로그는 새 임시 디렉터리에 남기고 그 경로를 출력한다. 자동으로 사용자 파일을 삭제하지 않는다.

## 실제 FTP + KIOFuse smoke — 선택 실행

실제 데스크톱 세션의 KIOFuse와 FTP KIO worker가 필요하다. 이 검사는 모의 D-Bus 세션이 아니라 해당 사용자 세션에서 실행한다. 별도 가상 환경에 테스트 의존성을 설치한다:

```bash
python3 -m venv /tmp/opencode/ncat-ftp-venv
/tmp/opencode/ncat-ftp-venv/bin/pip install pyftpdlib==2.2.0
/tmp/opencode/ncat-ftp-venv/bin/python tools/network-ftp-smoke.py --image ./nCatImage-0.4.3-x86_64.AppImage
```

도구가 `127.0.0.1`의 임의 포트에서 **합성 파일만 제공하는 익명 읽기 전용 FTP 서버**를 만들고, 폴더·CBZ 각각 500회 탐색을 검사한다. 종료 시 자신의 서버를 닫는다. OS 연결 도구의 마운트/캐시 정리는 OS 정책을 따르며 사용자 KIOFuse 서비스를 중지하거나 전체 마운트를 해제하지 않는다. 외부 서버·실제 계정·실제 책에 접속하지 않는다.

일반 FTP는 암호화되지 않는다. 이 루프백 검사를 SFTP·SMB 실제 인증, GNOME/GVfs 전체 경로, 느린/끊긴 WAN, 대형 책 성능의 통과 기록으로 해석하지 않는다.
