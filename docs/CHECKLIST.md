# 릴리스 전 실물 점검 (Windows)

CI가 확인할 수 없는, 실제 Windows PC에서만 볼 수 있는 항목입니다. 릴리스마다 한 번 훑어봅니다.
`[ ]`를 `[x]`로 바꾸고, 문제가 있으면 이슈로 남깁니다.

## 설치·실행
- [ ] zip을 풀고 `muud-view.exe` 실행 → 창이 1초 안에 뜬다
- [ ] SmartScreen 경고가 뜨면 "추가 정보 → 실행"으로 실행된다
- [ ] 회사 PC에서 백신/EDR이 막지 않는다 (막으면 IT에 README의 검증 정보 전달)

## 보기
- [ ] 125% / 150% 배율 모니터에서 `0`(100%)이 실제 픽셀 1:1로 보인다
- [ ] 세로로 찍은 휴대폰·카메라 사진이 똑바로 선다 (EXIF 회전)
- [ ] 큰 카메라 JPEG(30MP 이상) 폴더에서 `→`를 눌러 넘길 때 버벅이지 않는다
- [ ] 한글 파일명·한글 폴더 경로가 정상 표시된다
- [ ] 움직이는 GIF가 재생된다

## 키보드
- [ ] **한글 입력기를 켠 상태(한/영 한글)** 에서 `F` `B` `I` `U` `M` `N` `X` `Z` 단축키가 동작한다
- [ ] `Ctrl+C` 후 탐색기에 `Ctrl+V` → 파일이 붙는다
- [ ] `Ctrl+C` 후 카카오톡/팀즈/슬랙 등에 `Ctrl+V` → 그림이 붙는다
- [ ] `Ctrl+Alt+C` → 작은 그림이 붙는다
- [ ] 캡처 도구(`Win+Shift+S`) 후 `Ctrl+V` → 캡처 화면이 보인다
- [ ] `Insert` / `Ctrl+Insert` / `Shift+Insert` / `Alt+3` → 사진 폴더에 복사·이동된다
- [ ] `Delete` → 휴지통, `Ctrl+Z` → 원래 자리로 돌아온다
- [ ] `Ctrl+Enter` → 탐색기가 열리고 파일이 선택돼 있다

## Windows 통합 (`F5`)
- [ ] `1 SET AS IMAGE VIEWER` → Windows 기본 앱 설정이 열리고 muud view를 고를 수 있다
- [ ] 이미지 우클릭 → "muud view로 열기"가 있다 (Windows 11은 "더 많은 옵션 표시" 안)
- [ ] 폴더 우클릭 → "muud view로 폴더 보기"가 있다
- [ ] `2 ADD START MENU SHORTCUT` → 시작 메뉴 검색에 나온다
- [ ] 창이 열린 상태에서 탐색기로 다른 사진을 열면 **새 창 없이** 기존 창에서 열린다
- [ ] `3 REMOVE FROM WINDOWS` → 위 항목이 모두 사라진다
- [ ] exe 옆에 `portable` 파일을 만들면 설정이 `muud-view-data` 폴더에 저장된다

## 지원
- [ ] `4 COPY DIAGNOSTICS` → 버전·GPU·현재 파일·로그가 복사된다
- [ ] `5 OPEN LOG FOLDER` → `log.txt`가 있는 폴더가 열린다
