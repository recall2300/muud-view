# 성능 측정

## 방법

| 무엇 | 명령 |
|---|---|
| muud view (zune-jpeg) | `MUUD_BENCH_JPEG=파일.jpg cargo test --release -- --ignored --nocapture bench_jpeg` (파일을 안 주면 합성 32.5MP) |
| libjpeg-turbo 비교 | `python bench/compare_libjpeg_turbo.py 파일.jpg …` (Pillow에 들어 있는 libjpeg-turbo 사용, 앱에는 포함되지 않음) |
| 합성 테스트 파일 | `python bench/compare_libjpeg_turbo.py --synthetic 폴더` → 6960×4640(R6 Mark III 크기) 4:2:0 / 4:2:2 |
| 큰 GIF 첫 프레임 | `cargo test --release -- --ignored --nocapture bench_gif` |
| CI 속도 회귀 검사 | `cargo test --profile bench-ci -- --ignored --nocapture bench_ci` |

## zune-jpeg vs libjpeg-turbo (2026-11-24)

2코어 리눅스 서버, 같은 파일, 5회 중앙값. libjpeg-turbo 3.1.4.1.
실제 R6 Mark III 파일은 아직 없어서 같은 크기의 합성 사진(그라데이션 + 센서 노이즈, 품질 95)으로 측정 — 실물 파일로 다시 재서 아래 표에 추가할 것 ([CHECKLIST](CHECKLIST.md)).

| 파일 | libjpeg-turbo 원본 | libjpeg-turbo 1/4 축소 디코딩 | muud view 원본 | muud view 화면 크기 (2160×1440) |
|---|---|---|---|---|
| 합성 32.5MP 4:2:0, 11.1MB | 247ms | 167ms (1740×1160) | 378~387ms | 444~496ms |
| 합성 32.5MP 4:2:2, 11.2MB | 260ms | 180ms (1740×1160) | 348~399ms | 517~536ms |

**해석**
- 원본 디코딩은 libjpeg-turbo가 약 1.5배 빠름 (SIMD 허프만·IDCT).
- 화면용으로는 차이가 더 큼: libjpeg-turbo는 IDCT 단계에서 1/2·1/4·1/8로 줄여 디코딩할 수 있어서, 전체 디코딩 후 줄이는 muud view보다 약 3배 빠름.
- muud view 쪽 시간에는 EXIF·히스토그램이 들어 있지만 몇 ms 수준.

**→ 2026-11-25 libjpeg-turbo로 전환함** (아래 "전환 후"). 당시 메모: libjpeg-turbo를 쓰려면 Windows 빌드에 C 컴파일러·NASM·CMake가 필요하고(현재는 순수 Rust), 라이선스(IJG + BSD-3-Clause + Zlib)를 `deny.toml`에 추가하고 고지문을 넣어야 함. 대신 화면 크기 디코딩이 장당 약 300ms 빨라짐. 순수 Rust를 유지한다면 zune-jpeg에 축소 IDCT가 생길 때까지 지금 구조(코어 수 디코딩 + 미리 읽기)로 체감 속도를 보완.

## 전환 후 (2026-11-25, libjpeg-turbo 3.1.0 정적 링크)

같은 2코어 서버, 같은 파일, 3회. 화면 크기는 2560×1440 모니터 기준(2160×1440).

| 파일 | 원본 (전 → 후) | 화면 크기 (전 → 후) |
|---|---|---|
| 합성 32.5MP (테스트 생성) | 340~390ms → 269~276ms | 460~520ms → 214~241ms |
| 합성 32.5MP 4:2:0 | 378~387ms → 239~263ms | 444~496ms → 188~209ms |

- 원본(현재 사진과 앞쪽 3장 — 확대용 원본을 함께 보관): libjpeg-turbo 전체 디코딩 + 축소
- 화면 크기만(키를 누르고 넘길 때, 뒤쪽 사진, 썸네일): 디코딩 단계에서 n/8로 줄인 뒤(32.5MP → 3/8) 나머지만 축소 → 약 2.3배
- 썸네일(256px)은 1/8 디코딩
- CMYK·무손실·손상된 JPEG는 zune-jpeg로 (예전과 같은 결과)

## CI 기준값

`bench/baseline.txt` — windows-latest 러너에서 (작업 시간 ÷ 보정 루프 시간). 30% 넘게 느려지면 "Speed" 작업 실패.

| 작업 | CI 시간 | 비율 |
|---|---|---|
| 32.5MP JPEG 원본 디코딩 | 212ms | 3.534 |
| 32.5MP JPEG 화면 크기 디코딩 | 287ms | 4.782 |
| 32.5MP → 2160×1440 축소 | 79ms | 1.310 |

## 기타 측정

| 항목 | 결과 |
|---|---|
| 장당 메모리 (32.5MP, 2560×1440 모니터) | 129.2MB → 12.4MB |
| ICC 변환 (32.5MP) | 화면 크기 +25ms, 원본 +270ms |
| 큰 GIF (1200×900 · 120프레임 · 123MB) | 첫 프레임 25ms, 전체 3.7초 |
