# 지원 포맷

| 종류 | 확장자 | 방법 |
|---|---|---|
| 일반 이미지 | jpg jpeg jfif png apng gif webp bmp tif tiff tga ico pnm qoi dds hdr exr | `image` 크레이트. JPEG는 libjpeg-turbo(SIMD, 화면용은 디코딩 단계에서 축소), CMYK·무손실·손상된 JPEG는 zune-jpeg |
| JPEG XL | jxl | jxl-oxide (MIT OR Apache-2.0, 순수 Rust). 손실·무손실·투명·흑백·애니메이션, 넓은 색역·ICC는 sRGB로 변환(moxcms), Exif 정보 표시. JPEG보다 느림 — 12MP 한 장에 2코어 서버 약 0.8초 (미리 읽기가 가려 줌) |
| AVIF | avif | AV1 디코더는 rav1d (BSD-2-Clause, dav1d를 Rust로 옮긴 것), HEIF 컨테이너는 직접 읽음(`core/avif.rs`). 8·10·12비트, 4:2:0·4:2:2·4:4:4·흑백, 투명, 타일 격자(grid), 회전·뒤집기(irot·imir), CICP 색(Display P3·BT.2020 → sRGB)·ICC, Exif. 12MP 한 장 약 0.37초(2코어 서버). 움직이는 AVIF는 대표 정지 화면만, HDR(PQ·HLG)은 톤 매핑 없이 |
| SVG | svg | resvg (Apache-2.0 OR MIT). 보이는 크기로 그려서 확대해도 선명 (원본 화질은 긴 변 2048px 이상) |
| Photoshop | psd | psd 크레이트 (MIT OR Apache-2.0). 파일에 저장된 **합성 이미지**만 — 레이어는 보지 않음. "호환성 최대화"를 끄고 저장한 PSD는 합성 이미지가 비어 있을 수 있음 |
| 카메라 RAW | cr2 cr3 crw nef nrw arw srf sr2 dng raf orf rw2 pef srw rwl 3fr iiq x3f erf mrw | 카메라가 넣어 둔 **내장 JPEG 미리보기** 중 가장 큰 것 (대부분 원본 크기), RAW의 EXIF 회전 적용. RAW 현상은 하지 않음 |
| 압축파일 | zip cbz | 풀지 않고 폴더처럼 ([PROGRESS](PROGRESS.md) M25) |

## HEIC / AVIF 검토

**AVIF — 2026-11-25 지원.** 처음 검토 때 막혔던 두 가지가 풀림
- 컨테이너: `image` 크레이트가 쓰는 `mp4parse`는 MPL-2.0 → HEIF 상자(meta·iinf·iloc·iprp·iref·idat)를 직접 읽는 파서를 씀(약 400줄)
- AV1 디코더: dav1d(C, meson 빌드) 대신 **rav1d**(BSD-2-Clause, 같은 코드의 Rust 판). x86 어셈블리에 NASM이 필요한데 libjpeg-turbo 때문에 이미 갖춤. AV1은 로열티 없음
- 주의: rav1d 1.1은 일부 손상 파일에서 C 함수 경계 안에서 패닉하는데 이건 잡을 수 없어 **앱이 꺼짐**. 디코더를 프레임 문맥 2개 이상으로 설정하면 그 경로를 타지 않음(깨진 파일 432건을 하나씩 따로 실행해 0건 확인). 그 위에 OBU 구조·시퀀스 헤더를 미리 확인

**HEIC — 아직 넣지 않음.** 라이선스·특허 때문

| | 디코더 | 걸림돌 |
|---|---|---|
| HEIC (아이폰) | libheif + libde265 | 둘 다 **LGPL** — 정적 링크로 단일 exe를 만들 수 없음. HEVC는 **특허 풀**(MPEG LA·Access Advance) 대상이라 배포자가 로열티 문제를 질 수 있음. 순수 Rust HEVC 디코더(`heic-decoder` 등)도 특허 문제는 같음 |

**할 수 있는 방법 (정책 안에서)**: **Windows 이미징 구성 요소(WIC)** 로 디코딩 — 사용자가 Microsoft Store의 "HEIF 이미지 확장"+"HEVC 비디오 확장"을 설치했다면 Windows가 디코딩 → exe에 코덱이 없어 라이선스·특허 부담 없음. 회전은 WIC가 알려 주는 HEIF 방향 값을 적용해야 함. CI(Windows Server)에는 이 확장이 없어 실제 HEIC 디코딩은 사람이 확인해야 함

그 전까지는 아이폰 설정 → 카메라 → 포맷 → "높은 호환성"(JPEG)이나, 사진을 옮길 때 자동 변환을 쓰는 것을 안내.
