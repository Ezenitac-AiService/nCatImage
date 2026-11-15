# 05. 메모리와 반응성 최적화

v0.4.3 · 2026-11-16. 아래 초기 목표와 08의 과거 실측·현재 확인 범위를 구분한다.

이 문서의 수치는 Linux 초기 정책/목표이며 아직 측정되지 않았다. “가볍다”는 **정상 만화를 읽으면서 적은 live bytes·낮은 유휴 CPU·짧은 입력 지연**을 함께 만족해야 한다. 숫자만 낮춰 페이지를 거부하는 것은 최적화가 아니다.

현재 M1은 전체 목표를 완료한 것이 아니라 예약/lease·단일 worker의 full-decode·8MiB preview 경로부터 구현했다. [08의 실제 결과/차이](08-implementation-status.md)를 함께 확인한다. 아래 목표를 단발 smoke 결과와 혼동하지 않는다.

현재 폴더(현재 디렉터리만)·ZIP/CBZ의 render LRU와 인접 페이지 1개 prefetch가 구현되었다. 기본 24MiB 픽셀/키 상한과 전역 예약을 동시에 지키고 표시 lease는 퇴출 후에도 유효하다. 원본/썸네일 cache·전체 압력 hysteresis는 목표 단계다. 조정 가능한 실행 설정은 [README](../README.md)를 따른다.

## 1. 우선 제거할 비용

1. 원본/압축파일 전체 RAM 적재와 전체 책 선행 decode.
2. 화면보다 큰 원본을 매번 GUI로 전달하는 copy/upload.
3. 현재 선택과 무관한 prefetch/썸네일·zoom 중간 결과.
4. 장수 기준 무제한 cache·고정 animation cache·여러 worker/runtime/thread pool.
5. 정지 화면의 redraw·timer polling·반복 목록 scan·무거운 초기 서재.

품질/안전을 낮추기 전에 불필요한 할당·초기화·복사·중복 작업부터 줄인다. 전송 픽셀의 크기와 실제 decode workspace는 따로 계산한다.

## 2. 프로세스 합산 예산

| 모드 | 이미지/입력/인덱스/작업/표시 합산 예산 | 사용 방식 |
|---|---:|---|
| 절약 | 64MiB | 다음 페이지 준비/thumbnail 최소, 큰 이미지 거부 가능 |
| 기본 | **192MiB** | 기본 만화 corpus가 정상 동작해야 하는 기준 |
| 큰 이미지 | 384MiB | 사용자 명시 선택·고해상도/전체 decode 대응, 지원 보장 아님 |

한 GUI 프로세스의 모든 책/창과 연결 worker를 합산한다. 현재 기본은 한 읽기 창이며 다중 창을 구현하더라도 예산을 창마다 복제하지 않는다. 별도 앱 프로세스 사이에는 자동 전역 상한을 보장하지 않는다.

예산에는 decode/색변환 workspace·현재/다음 pixels·입력 buffer·IPC/shared mapping·index/meta·추정 texture/staging을 포함한다. 동일 공유 물리 할당은 중복 과금하지 않지만 실제 copy는 별도다. GTK/폰트/library mapping/driver 전체 RSS와는 다르며 07에서 프로세스 트리 RSS/PSS도 측정한다.

0.4.3의 KIOFuse/GVfs 데몬·OS 네트워크 캐시는 앱 밖의 비용으로 192MiB 예약에 포함되지 않는다. FUSE가 파일 전체를 캐시할 수 있으므로 네트워크 읽기 메모리/디스크 측정에는 관련 데몬·OS cache를 별도로 기록한다. 앱이 별도 다운로드하지 않는다는 사실을 전체 시스템 자원 상한으로 해석하지 않는다.

우선순위는 **현재 표시 → 다음 전경의 피크 작업 공간 → 현재 spread의 다른 패널 → 실제 보이는 썸네일 → 재사용 cache/미리 읽기**다. 다음 전경의 필수 작업을 cache 때문에 거부하지 않는다.

### 기본 모드의 선택적 cache 상한

| cache | soft ceiling |
|---|---:|
| 원본/단계 decode 재사용 | 48MiB |
| 현재 외 페이지 크기 preview/render | 24MiB |
| 썸네일 | 8MiB, 최대 96개 |

합계 80MiB는 보장 할당량/채울 목표가 아니다. 전경 예약 뒤 남은 부분에서만 쓴다. 현재 표시/필수 workspace는 별도로 추적하되 전체 192MiB 안에 있어야 한다. 절약/큰 모드는 byte cap을 비율 조정하며 최소 캐시 장수는 강제하지 않는다.

- 예약 사용량 80%에서 새 비필수 작업 중단, 90% 또는 전경 예약 실패 시 cache 회수, 65% 아래+전경 대기 없음에서만 재개해 진동을 줄인다.
- 창 최소화/완전 비가시 상태는 prefetch/thumbnail을 취소한다. 표시 복원에 필요한 작은 buffer 외 재사용 cache는 회수 가능하다. 단순 포커스 이동은 비가시와 구분한다.
- 기본 texture만으로 표시를 유지할 수 있으면 원본 CPU cache를 먼저 버린다. 확대 시 재decode 비용과 기본 탐색 메모리 절약의 균형을 계측한다.

## 3. 비용 계산과 고해상도 한계

픽셀 bytes만 계산해 성공으로 판단하지 않는다. `checked_mul/add`·검사된 stride/채널·`TryFrom` 길이 변환으로 **실제 선택 페이지의 최악 동시 생존 buffer**를 계산한다. 모든 large allocation 전에 parent permit을 얻는다.

| 크기 | RGBA8 1개 | RGBA16 1개 | RGBA32F 1개 |
|---|---:|---:|---:|
| 4MP | 약 15MiB | 약 31MiB | 약 61MiB |
| 12MP | 약 46MiB | 약 92MiB | 약 183MiB |
| 48MP | 약 183MiB | 약 366MiB | 약 732MiB |
| 150MP | 약 572MiB | 약 1,144MiB | 약 2,289MiB |

- 150MP는 입력 검사 상한이지 표시/1:1 보장이 아니다. 150MP JPEG는 축소 decode로 표시될 수 있지만 전체 RGBA8을 요구하는 PNG는 큰 이미지 모드에서도 거부될 수 있다.
- 48MP PNG도 기본 모드에서는 원본 하나에 거의 모든 예산이 필요하므로 추가 색변환/출력을 포함하면 거부될 수 있다. 사용자에게 큰 이미지 모드/지원 가능한 축소 경로를 안내한다.
- 16bit/float 원본은 SDR RGBA8 표시보다 workspace가 크다. 색변환·resize를 가능한 row/tile 단위로 처리하되 codec이 전체 buffer를 요구하면 그 비용을 숨기지 않는다.
- `try_reserve` 등 실패 가능한 allocation을 활용하되 외부 codec OOM/abort는 worker 종료로 복구한다. 앱 예약과 Linux hard limit/프로세스 실측을 병행한다.

예약 실패 순서: 비필수 작업 취소 → 불필요한 CPU/render cache 회수 → 실제 지원되는 축소 decode/viewport 출력 → 필요 시 이전 표시 해제/중립 로딩 → 명시적 거부. 자동으로 사용자 메모리 한도를 올리지 않는다.

### 전환 피크 실험

| 상황 | 포함할 live 자원 | 정책 |
|---|---|---|
| 일반 page A→B | A texture·B decode/workspace·출력/IPC·B texture/staging | A 원본 cache 먼저 해제·B는 화면 크기 출력 |
| 두 장 보기 | 두 표시·한 worker의 순차 decode·교체 중 pixels | 한 패널 먼저 정상 표시·작업 직렬화·공통 예산 |
| zoom/DPI 갱신 | 현재 texture·새 단계/crop·IPC/staging | 최신 작업 1개·swap 후 구 texture 해제 |
| 가로 분할 | 같은 물리 원본·반쪽 출력/anchor | 두 반쪽 때문에 원본을 두 번 보관하지 않음 |
| 연속 보기 | viewport 주변 pixels·한도 있는 layout/index | offscreen 원본/texture 퇴출·anchor 유지 |

PoC는 buffer 생존 도식·copy 횟수·예약/실측 차이를 기록한다. swap 때 두 texture가 잠시 공존하는 비용이나 GDK의 지연 upload를 빠뜨리지 않는다.

## 4. 디코딩·렌더·업로드

- 맞춤은 물리 viewport 크기 기준으로 충분한 최소 해상도를 선택한다. JPEG의 실제 축소 decode를 활용하고 다른 형식에 같은 능력이 있다고 가정하지 않는다.
- decode 목표는 긴 변 **512/1024/2048/4096/원본** 단계에서 선택한다. 종횡비/실제 축소 비율/예산이 우선이며 crop 경로가 더 싸면 crop을 쓴다.
- zoom/resize 입력은 즉시 기존 texture 변환에 반영하고 정밀 갱신은 최신 1개로 병합한다. 입력 안정 100ms 뒤 갱신, 계속 조작해도 최대 250ms마다 필요 해상도를 재평가하되 동시 대기 작업을 늘리지 않는다.
- 같은 decode 단계/원본으로 충분하면 재decode하지 않는다. 일회성 zoom의 모든 출력 크기를 render cache에 넣지 않는다.
- GUI로 보내는 것은 기본 viewport용 RGBA8 또는 검증된 native 포맷이다. 큰 원본 1:1 패닝은 제한 crop/타일을 사용한다. 원본 전체 decode 필요 여부는 별개다.
- UI dispatch는 texture 최대 1개, 초기 전송 payload **8MiB 이하**를 admission 기준으로 시험한다. 큰 화면/두 패널은 preview→나눠 반영/타일 표시로 처리하고 같은 dispatch에서 여러 조각을 몰아넣어 우회하지 않는다.
- 8MiB는 지원 해상도 상한이 아니라 dispatch당 분할 목표다. 4K 최종 표시를 영구 저해상도로 대체하지 않는다. GTK의 실제 지연 upload/frame hitch를 보고 분할 방식·크기 기준을 확정한다.
- texture 최대 차원·alpha/색 형식·stride 정렬·소프트웨어 renderer를 검사한다. 미지원 경로는 안전한 표시 오류이며 무한 재시도를 하지 않는다.

## 5. cache key·미리 읽기·압축

`DecodeKey = source/book epoch + content revision + entry ID + TIFF page + 실제 decode 단계 + 색/codec 정책 버전`이다. `RenderKey`는 여기에 읽기 반쪽/crop·출력 크기·보기 회전을 더한다. 같은 key라도 무효화 완료 전 작업은 삽입 시 현재 리비전을 다시 검사한다.

- 기본은 **다음 논리 페이지/spread의 화면 크기 출력 한 개**만 미리 준비한다. 직전 페이지는 byte cap 안에서 재사용할 수 있다. 여러 장의 full-resolution 원본을 기본으로 보관하지 않는다.
- 방향이 바뀌면 대기 후보를 즉시 바꾸고 현재 전경을 우선한다. 빠른 건너뛰기 동안 중간 페이지/썸네일은 decode하지 않는다.
- 한 worker에서 비필수 작업은 전경 idle 때 한 개만 실행한다. prefetch가 입력을 막는 비용이 이익보다 크면 해당 codec/solid 소스에서 자동 비활성화하고 다음 페이지를 사용자 요청 시만 읽는다.
- ZIP/비solid와 solid 압축의 prefetch 전략을 분리한다. solid 재해제 비용·임시 cache 사용은 04/06의 bytes/time/disk 한도 안에서 결정한다.
- 썸네일은 줄을 펼쳤을 때 보이는 항목만 읽고 닫으면 대기 요청을 취소한다. 기본 지속 디스크 thumbnail cache는 없다.
- 이미지 raw cache보다 작은 preview를 우선한다. key의 충분 해상도 호환 검색과 정확한 동일성을 구분한다. 오래된 file overwrite/reopen을 같은 내용으로 추정하지 않는다.

## 6. 지속 계측

기능마다 request→queue→read/extract→decode→color/resize→IPC→GUI→실제/대체 제시 시간을 기록한다. logger는 기본 비활성/정제·유한 buffer이며 경로/원본을 남기지 않는다.

필수 카운터: live/예약 bytes·cache별 bytes·copy/upload bytes·worker/codec thread 수·취소/kill/restart·prefetch hit/폐기·offscreen thumbnail·열린 handle/temp bytes·유휴 wakeups. 목표와 실제 RSS/PSS/CPU/지연의 차이는 07 기준으로 판정한다.

현재 AppImage는 호환성을 위해 Cairo를 기본 renderer로 사용하고 환경변수로 변경할 수 있다. 이것이 가장 빠르거나 가볍다는 비교 결론은 아니다. 네트워크의 500회 합성 smoke와 작은 파일 warm cache는 LAN/WAN 지연·실제 corpus의 p95·전체 메모리 plateau 게이트를 대체하지 않는다.
