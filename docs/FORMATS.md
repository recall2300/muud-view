# 지원 포맷

| 종류 | 확장자 | 방법 |
|---|---|---|
| 일반 이미지 | jpg jpeg jfif png apng gif webp bmp tif tiff tga ico pnm qoi dds hdr exr | `image` 크레이트. JPEG는 libjpeg-turbo(SIMD, 화면용은 디코딩 단계에서 축소), CMYK·무손실·손상된 JPEG는 zune-jpeg |
| SVG | svg | resvg (Apache-2.0 OR MIT). 보이는 크기로 그려서 확대해도 선명 (원본 화질은 긴 변 2048px 이상) |
| Photoshop | psd | psd 크레이트 (MIT OR Apache-2.0). 파일에 저장된 **합성 이미지**만 — 레이어는 보지 않음. "호환성 최대화"를 끄고 저장한 PSD는 합성 이미지가 비어 있을 수 있음 |
| 카메라 RAW | cr2 cr3 crw nef nrw arw srf sr2 dng raf orf rw2 pef srw rwl 3fr iiq x3f erf mrw | 카메라가 넣어 둔 **내장 JPEG 미리보기** 중 가장 큰 것 (대부분 원본 크기), RAW의 EXIF 회전 적용. RAW 현상은 하지 않음 |
| 압축파일 | zip cbz | 풀지 않고 폴더처럼 ([PROGRESS](PROGRESS.md) M25) |

## HEIC / AVIF 검토 (2026-11)

**결론: 지금은 넣지 않는다.** 라이선스 정책(MIT·Apache·BSD·Zlib·OFL만)과 특허 문제 때문.

| | 디코더 | 걸림돌 |
|---|---|---|
| HEIC (아이폰) | libheif + libde265 | 둘 다 **LGPL** — 정적 링크로 단일 exe를 만들 수 없음. HEVC는 **특허 풀**(MPEG LA·Access Advance) 대상이라 배포자가 로열티 문제를 질 수 있음 |
| AVIF | dav1d (BSD-2) + 컨테이너 파서 | `image` 크레이트의 AVIF 디코딩은 컨테이너 파서 `mp4parse`가 **MPL-2.0**이라 정책 위반. dav1d는 C·어셈블리 빌드(meson·nasm) 필요. AV1 자체는 로열티 없음 |

**나중에 할 수 있는 방법 (정책 안에서)**
- **Windows 이미징 구성 요소(WIC)** 로 디코딩: 사용자가 Microsoft Store의 "HEIF 이미지 확장"·"AV1 비디오 확장"을 설치했다면 Windows가 디코딩해 줌 → 우리 exe에는 코덱이 들어가지 않아 라이선스·특허 부담 없음. COM 인터페이스(IWICImagingFactory 등)를 직접 선언해야 해서 작업량이 있음
- AVIF는 순수 Rust AV1 디코더가 쓸 만해지고, MPL이 아닌 컨테이너 파서가 생기면 다시 검토

그 전까지는 아이폰 설정 → 카메라 → 포맷 → "높은 호환성"(JPEG)이나, 사진을 옮길 때 자동 변환을 쓰는 것을 안내.
