# 04. 이미지 형식·압축 지원

v0.4.3 · 2026-11-16. 신규 네트워크 입력은 코덱/압축 지원 형식을 늘리지 않는다.

## 1. 지원의 정의

확장자 목록만 길게 적지 않는다. **Linux release 패키지의 특정 codec/feature 조합에서 정상 corpus·색/방향·크기/오류/메모리 검사를 통과한 형식**만 정식 지원이라고 표시한다. M1에는 기본 정지 코덱/ZIP의 부분 구현과 합성 입력 회귀가 있으나 전체 matrix/색관리 corpus 인증은 아직 없다. [08](08-implementation-status.md)에 현재 범위를 구분한다.

기본 설치가 아래 P0/P1을 단계적으로 포함하도록 계획한다. 코덱 자동 다운로드나 실행 중 플러그인 설치는 하지 않는다. 추가 코덱 때문에 기본 JPEG/PNG startup이 느려지면 초기화·링크/패키지 구성을 재검토한다.

## 2. 정지 이미지 matrix

| 형식 | 단계 | 범위/제한 | Rust 측 처리 후보 |
|---|---|---|---|
| JPEG/JPG | P0 | baseline/progressive·EXIF·CMYK/ICC·검증된 축소 decode | image/하위 JPEG decoder, 필요 시 좁은 native adapter 비교 |
| PNG | P0 | alpha·interlace·16bit→SDR 표시, APNG 기본 이미지는 별도 검사 | png/image |
| WebP | P0 | lossy/lossless·alpha·첫 합성 frame | image-webp/image, 필요 시 libwebp |
| GIF | P0 | 첫 유효 합성 frame 정지 표시 | gif/image |
| BMP | P0 | 일반 Windows BMP corpus, 모든 희귀 변형 미보장 | image |
| AVIF | P1 | 정지 SDR 변환·고비트 입력 검사, animation 제외 | libavif 등 Rust adapter 후보 비교 |
| JPEG XL/JXL | P1 | 정지 SDR 변환·고비트 입력 검사, animation 제외 | libjxl 또는 검증된 Rust decoder 후보 |
| TIFF | P1 | 다중 페이지·선택 page별 크기/방향·압축 방법별 표 | tiff/개별 API |
| QOI/TGA | P1 | 정지 이미지·alpha/방향 corpus | image/전용 crate |
| HEIF/HEIC | P2 | native 패키징·색/특허/지역 조건 확인 후 개별 승인 | libheif 등 후보, 미선정 |
| JPEG2000/PSD/RAW 등 | P2 | flatten/preview/기종별 범위를 따로 정의, 무조건 지원하지 않음 | 개별 비교 후 결정 |

`image`의 feature 이름에 포맷이 있다고 decode·축소·다중 페이지·ICC·animation을 전부 지원한다고 간주하지 않는다. default feature는 끄고 필요한 decoder만 활성화한다. 버전·실제 하위 backend·thread 수·native dependencies를 기록한다. 이미 지원하는 형식에 검증 안 된 fallback을 연속 실행하지 않는다.

### 애니메이션 경계

- P0/P1 기본 제품은 정지 코믹 감상이다. GIF/WebP는 첫 합성 frame만, APNG는 정지 default image가 있으면 그 이미지를, 없으면 첫 합성 frame을 표시하는 경로를 검증한다. APNG 판별/정지 표시를 독립 capability로 기록한다.
- 전체 frame을 읽고 처음만 쓰지 않는다. animated AVIF/JXL은 정지 표시 경로도 별도 corpus 없이 지원으로 표기하지 않는다.
- 재생은 P2 별도 기능이다. 활성화 시 합성 canvas·restore 상태·timeline·loop·sliding window·최소 지연·중간 오류·반복 재디코딩 비용을 새 명세/게이트로 승인한다. 현재 192MiB 정책에 전체 animation cache를 미리 넣지 않는다.

## 3. 압축 matrix

| 컨테이너 | 단계 | 필수 검증 범위 |
|---|---|---|
| ZIP/CBZ | P0 | Stored/Deflate·ZIP64·중복 항목·UTF-8/레거시 이름·seek 읽기 |
| RAR/CBR | P0 | 비암호화/비분할 RAR4/5·검증된 solid/비solid method, 느린 임의 접근 안내 |
| 7Z/CB7 | P1 | 비암호화/비분할·LZMA/LZMA2 및 검증된 method·solid 경계 |
| TAR/CBT | P1 | 일반 tar 내부 이미지, index/seek·희소/링크 항목 거부 |
| tar.gz/tar.xz·LZH 등 | P2 | 순차 비용/한도·backend 지원·필요성 확인 후 |

- ZIP은 `zip` crate 우선, RAR/7Z는 libarchive Rust adapter 우선 PoC다. TAR는 작은 전용 Rust crate와 libarchive 재사용 비용을 비교한다. 외부 `unrar/7z` 실행 파일 호출을 기본 처리로 쓰지 않는다.
- CBZ/CBR/CB7/CBT는 컨테이너 별칭이지 별도 이미지 codec이 아니다. 실제 signature와 parser 결과를 사용한다.
- “RAR/7Z 지원”은 모든 버전/압축법 지원 보장이 아니다. 배포본에 method별 표를 포함하고 실패하면 “손상”과 “미지원 방식”을 구분한다.
- 암호화·분할·중첩 자동 탐색은 1.0 제외다. 링크/디렉터리/비이미지 항목은 읽기 페이지로 만들지 않는다.
- 원본 컨테이너 전체를 RAM에 올리지 않는다. item index와 제한 reader로 요청 항목만 읽으며 코덱이 seek를 요구하면 필요 시 상한 있는 임시 파일로 spool한다.
- solid는 앞선 data 해제가 필요할 수 있다. 순차 decode 위치/작은 임시 cache를 활용하되 **첫 표시부터 전체 압축을 해제하지 않는다.** 출력 제한과 함께 건너뛴 해제 bytes/CPU/시간도 추적한다.
- 전경 선택이 뒤로 크게 이동하면 재해제될 수 있다. 디스크 예산 내 임시 cache나 재읽기를 택하고 비용/취소 가능 상태를 보여 준다. 순차·랜덤 성능은 따로 판정한다.
- 원시 항목 index를 실제 ID로 사용하고 같은 이름도 합치지 않는다. 표시용 경로 정제와 원본 ID를 분리한다. entry 경로를 OS 추출 경로로 직접 사용하지 않는다.

## 4. 읽기 페이지·색·해상도

- 물리 이미지/컨테이너 항목과 읽기 페이지를 분리한다. TIFF page/가로 분할은 같은 입력에서 여러 논리 페이지를 만든다.
- 기본 목록 만들기는 확장자/MIME hint로 후보를 추리고 실제 표시 전에 signature를 검사한다. 책의 모든 파일을 열어 meta/decode를 완료한 뒤 첫 페이지를 표시하지 않는다.
- TIFF 내부 page 수는 한도 있는 뒤 작업으로 확정한다. UI는 미확정 총수를 “분석 중”으로 표시하고 ID/anchor를 유지한다. 불완전 번호로 임의 선택시키지 않는다.
- EXIF 방향은 파일/실제 선택 TIFF page별 적용한다. page0의 크기/방향을 다른 page에 재사용하지 않는다.
- 색관리는 **감상 정확성**이며 편집 기능이 아니다. embedded ICC/CMYK를 제한된 worker에서 SDR/sRGB로 변환한다. GUI에 색상 조절 slider나 저장을 제공하지 않는다.
- renderer 계약은 색공간·RGBA/BGRA·비트 깊이·straight/premultiplied alpha·stride를 명시한다. GUI가 요구하는 표현으로 한 번만 변환하고 리사이즈의 투명 경계/linear-light 처리 품질을 검사한다.
- 모니터 ICC/광색역/HDR 출력은 현재 보장하지 않는다. 시스템 compositor 보정과 앱 변환의 중복도 Linux에서 확인한다.
- JPEG는 실제 API의 축소 decode를 활용한다. PNG/TIFF/기타는 부분/축소 지원이 없으면 전체 decode 비용을 예약한다. GPU viewport crop이 원본 전체 decode 비용까지 없애지는 않는다.

## 5. 포맷별 통과 기록

각 형식/컨테이너마다 decoder 버전·Cargo feature·native build 옵션·정상/실패 corpus hash·decode 목표 크기·최대 live bytes·CPU·배포 의존성·알려진 제한을 남긴다. P0/P1 실패는 지원 범위 변경 승인을 받으며 메뉴만 제공하거나 다른 프로그램 설치를 요구하는 상태를 완료로 보지 않는다.

참고 자료: [image](https://github.com/image-rs/image) · [zip](https://github.com/zip-rs/zip2) · [libarchive](https://www.libarchive.org/) · [꿀뷰 공개 지원 목록](https://www.bandisoft.com/honeyview/). 특정 dependency 버전/기능을 이 문서 작성만으로 검증한 것은 아니다.

## 6. 네트워크 소스와 검증 범위

| 소스 | 현재 경로 | 확인 범위 |
|---|---|---|
| CIFS/sshfs 등 OS 로컬 마운트 | 기존 파일/폴더/ZIP worker 경로 | 마운트 제공자의 POSIX 동작에 의존 |
| FTP URI | KIOFuse 우선, GVfs/FUSE fallback | 루프백 익명 FTP의 합성 폴더·CBZ 및 각각 500회 탐색 확인 |
| SMB / SFTP URI | 같은 OS 브리지 | 모의 D-Bus의 라우팅·드롭·표시·취소 확인; 실제 서버 인증은 미검증 |
| GNOME GVfs | GIO 원격 백엔드와 FUSE 필요 | 구현됨; 전체 end-to-end 및 AppImage/시스템 모듈 조합 미검증 |

프로토콜 인증·호스트 키·캐시·재연결은 OS 도구에 맡긴다. FTP는 평문이며 HTTP/WebDAV/FTPS는 범위 밖이다. 앱은 네트워크 책을 통째로 RAM에 읽거나 미지원 형식을 외부 실행 파일로 우회하지 않는다. OS 도구는 seek를 위해 파일 전체를 별도 캐시할 수 있다. 로컬·원격 모두 현재 정지 JPEG/PNG/BMP/WebP·GIF 첫 frame과 ZIP/CBZ의 제한이 같다.
