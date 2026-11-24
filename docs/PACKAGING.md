# 배포·운영

## 릴리스

`Cargo.toml`의 버전을 올린 커밋이 `main`에 들어가면 Release 워크플로가 자동으로 테스트·빌드·패키징하고 GitHub 릴리스를 만듭니다 ([README](../README.md#개발)). 릴리스에 붙는 파일:

| 파일 | 내용 |
|---|---|
| `muud-view-vX.Y.Z-windows-x64.zip` | exe + 라이선스·고지문 |
| `muud-view-vX.Y.Z-windows-x64.exe` | exe 하나 |
| `SHA256SUMS.txt` | 해시 |
| `muud-view.json` | scoop 매니페스트 (이 릴리스의 해시가 들어 있음) |
| `recall2300.muudview*.yaml` | winget 매니페스트 3개 (version / installer / locale) |

빌드 출처 증명: `gh attestation verify <파일> --repo recall2300/muud-view`

## 업데이트 확인

F5 → **6 CHECK FOR UPDATES** — 브라우저로 최신 릴리스 페이지를 엽니다. 앱에는 통신 코드가 없으므로(PLAN 원칙 3) 버전을 자동으로 비교하지 않고, 알림에 지금 버전을 보여 줍니다.

## winget

처음 한 번은 [winget-pkgs](https://github.com/microsoft/winget-pkgs)에 PR을 보내야 합니다 (저장소 소유자가 직접).

```
winget install wingetcreate
wingetcreate submit --token <GitHub 토큰> <릴리스에 붙은 yaml 3개가 있는 폴더>
```

이후 버전은 `wingetcreate update recall2300.muudview --version X.Y.Z --urls <zip 주소> --submit`. 포터블 zip(`NestedInstallerType: portable`)이라 설치하면 `muud-view` 명령이 생깁니다.

## scoop

두 가지 방법:

- **직접 버킷**: 별도 저장소(예: `recall2300/scoop-bucket`)의 `bucket/`에 릴리스의 `muud-view.json`을 넣으면 `scoop bucket add muud https://github.com/recall2300/scoop-bucket` → `scoop install muud/muud-view`. `checkver`·`autoupdate`가 들어 있어 scoop의 자동 갱신 도구로 다음 버전을 따라갈 수 있음
- **Extras 버킷**: [ScoopInstaller/Extras](https://github.com/ScoopInstaller/Extras)에 PR (사용자가 어느 정도 생긴 뒤)

scoop 설치는 포터블 모드(`portable.txt`)로 동작하고 설정 폴더 `muud-view-data`는 업데이트해도 유지됩니다.

## 코드 서명 — SignPath Foundation 검토 (2026-11)

SmartScreen 경고와 회사 백신의 오탐을 줄이는 가장 확실한 방법. 오픈소스 프로젝트에 무료로 인증서를 줍니다.

| 조건 | muud view |
|---|---|
| OSI 승인 라이선스, 독점 코드 없음 | ✅ MIT, 모든 의존성 허용 라이선스 (`cargo deny`) |
| 공개 저장소, CI가 소스에서 직접 빌드 | ✅ GitHub Actions `windows-latest`, 빌드 출처 증명까지 |
| 이전 릴리스가 있을 것 | ✅ v0.1 ~ |
| GitHub·SignPath 계정 2단계 인증 | 소유자 확인 필요 |
| README에 서명 정책 문구 | 승인 후 추가 |

- 인증서의 게시자는 **SignPath Foundation**으로 표시됨 (개인 이름이 아님)
- 승인되면 Release 워크플로에 SignPath GitHub Action 단계를 넣어, exe를 서명 요청 → 서명된 exe로 패키징. 서명 승인자(저장소 소유자)를 정해야 함
- 신청은 저장소 소유자가 직접: <https://signpath.org/apply>

**권장:** 사용자가 늘어 회사 PC 배포가 막히는 일이 생기면 신청. 그 전까지는 README의 해시·빌드 출처 증명으로 검증.
