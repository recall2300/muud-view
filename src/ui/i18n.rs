//! Korean / English UI text. Strings are written in English in the code and
//! looked up here; anything missing simply stays English.

use std::sync::atomic::{AtomicBool, Ordering};

static KOREAN: AtomicBool = AtomicBool::new(true);

pub fn set_korean(on: bool) {
    KOREAN.store(on, Ordering::Relaxed);
}

pub fn is_korean() -> bool {
    KOREAN.load(Ordering::Relaxed)
}

/// Translates `en` when the UI is Korean.
pub fn tr(en: &str) -> &str {
    if !is_korean() {
        return en;
    }
    KO.iter()
        .find(|(e, _)| *e == en)
        .map(|(_, k)| *k)
        .unwrap_or(en)
}

/// Translates a message that may end in details after " · " or ": ".
pub fn tr_msg(msg: &str) -> String {
    if !is_korean() {
        return msg.to_owned();
    }
    for sep in [" · ", ": ", " → "] {
        if let Some((head, tail)) = msg.split_once(sep) {
            return format!("{}{sep}{}", tr(head), tr(tail));
        }
    }
    tr(msg).to_owned()
}

const KO: &[(&str, &str)] = &[
    // help sections and rows
    ("SHORTCUTS", "단축키"),
    ("ANY KEY TO CLOSE", "아무 키나 눌러 닫기"),
    ("MOVE", "이동"),
    ("VIEW", "보기"),
    ("TRANSFORM", "변환"),
    ("FILE", "파일"),
    ("PHOTO FOLDERS", "사진보관함"),
    ("NEXT", "다음"),
    ("PREVIOUS", "이전"),
    ("FIRST / LAST", "처음 / 끝"),
    ("± 10 IMAGES", "± 10장"),
    ("RANDOM", "무작위"),
    ("SLIDESHOW / STOP", "슬라이드쇼 / 정지"),
    ("RECENT FOLDERS", "최근 폴더"),
    ("FIT WINDOW", "창 맞춤"),
    ("FIT WIDTH", "폭 맞춤"),
    ("ORIGINAL 100%", "원본 100%"),
    ("ZOOM", "확대 / 축소"),
    ("LOCK ZOOM", "배율 고정"),
    ("FULLSCREEN", "전체화면"),
    ("BACKGROUND", "배경"),
    ("ON-SCREEN INFO", "화면 표시"),
    ("INFO PANEL", "정보 패널"),
    ("MINIMAP", "미니맵"),
    ("COMPARE", "비교"),
    ("PIXEL INFO", "픽셀 정보"),
    ("PIXEL INFO ON", "픽셀 정보 켜기 · 클릭하면 HEX 복사"),
    ("PIXEL INFO OFF", "픽셀 정보 끄기"),
    ("COMPARE VIEW", "비교 보기"),
    ("SIDE BY SIDE", "나란히"),
    ("SLIDER", "슬라이드"),
    ("DIFFERENCE", "차이"),
    ("COMPARE OFF", "비교 끝"),
    ("NEED TWO IMAGES", "이미지가 두 장 이상 필요해요"),
    ("PIXELS DIFFER", "픽셀 다름"),
    ("IDENTICAL", "똑같음"),
    ("MINIMAP ON", "미니맵 켜기"),
    ("MINIMAP OFF", "미니맵 끄기"),
    ("PIN TOP / BOTTOM BAR", "상단 / 하단 바 고정"),
    ("ROTATE CW", "시계 방향 회전"),
    ("ROTATE CCW", "반시계 방향 회전"),
    ("180° / RESET", "180° / 원래대로"),
    ("FLIP H / V", "좌우 / 상하 뒤집기"),
    ("OPEN FILE / FOLDER", "파일 / 폴더 열기"),
    ("COPY IMAGE + FILE", "이미지 + 파일 복사"),
    ("COPY SMALL", "작게 복사"),
    ("VIEW CLIPBOARD", "클립보드 보기"),
    ("RENAME", "이름 바꾸기"),
    ("RECYCLE / DELETE", "휴지통 / 완전 삭제"),
    ("UNDO", "되돌리기"),
    ("SETTINGS", "설정"),
    ("EXIT", "종료"),
    ("COPY TO FOLDER 1", "보관함 1에 복사"),
    ("MOVE TO FOLDER 1", "보관함 1로 이동"),
    ("COPY TO FOLDER 2", "보관함 2에 복사"),
    ("COPY TO FOLDER 3", "보관함 3에 복사"),
    // settings
    ("GENERAL", "일반"),
    ("MOUSE", "마우스"),
    ("KEYS", "단축키"),
    ("FOLDERS", "보관함"),
    ("WINDOWS", "WINDOWS"),
    ("LANGUAGE", "언어"),
    ("THEME", "테마"),
    ("LIGHT", "밝게"),
    ("DARK", "어둡게"),
    ("RESUME WHERE I LEFT OFF", "보던 사진부터 이어 보기"),
    ("SORT BY", "정렬"),
    ("NAME", "이름"),
    ("DATE", "날짜"),
    ("SIZE", "크기"),
    ("ORDER", "순서"),
    ("ASCENDING", "오름차순"),
    ("DESCENDING", "내림차순"),
    ("INK", "검정"),
    ("PAPER", "종이"),
    ("GRAY", "회색"),
    ("SMOOTH SCALING", "부드럽게 확대"),
    ("STRETCH SMALL IMAGES", "작은 이미지 늘리기"),
    ("COLOR PROFILE", "색 프로파일"),
    ("TOP BAR", "상단 바"),
    ("BOTTOM BAR", "하단 바"),
    ("PINNED", "고정"),
    ("AUTO-HIDE", "자동 숨김"),
    ("OFF", "끄기"),
    ("ON", "켜기"),
    ("FULL", "상세"),
    ("HIDE CURSOR IN FULLSCREEN", "전체화면에서 커서 숨기기"),
    ("WHEEL", "휠"),
    ("NAVIGATE", "넘기기"),
    ("WHEEL PAST THE EDGE → NEXT", "끝에서 휠 → 다음 사진"),
    ("MIDDLE CLICK", "가운데 클릭"),
    ("DOUBLE CLICK", "더블클릭"),
    ("SIDE BUTTONS → PREV / NEXT", "옆 버튼 → 이전 / 다음"),
    ("NONE", "없음"),
    ("MAXIMIZE", "최대화"),
    ("100%", "100%"),
    ("FIT", "맞춤"),
    ("PRESS A KEY · ESC CANCEL", "새 키를 누르세요 · ESC 취소"),
    ("RESET ALL", "모두 초기화"),
    ("EXPORT", "내보내기"),
    ("IMPORT", "가져오기"),
    ("ADD", "추가"),
    ("RESET", "초기화"),
    ("CHANGE", "변경"),
    ("DEFAULT", "기본값"),
    ("SET AS IMAGE VIEWER", "기본 이미지 뷰어로 등록"),
    ("ADD START MENU SHORTCUT", "시작 메뉴 바로가기 추가"),
    ("REMOVE FROM WINDOWS", "Windows에서 등록 해제"),
    ("COPY DIAGNOSTICS", "진단 정보 복사"),
    ("OPEN LOG FOLDER", "로그 폴더 열기"),
    ("SUPPORT", "지원"),
    ("REGISTERED", "등록됨"),
    ("NOT REGISTERED", "등록 안 됨"),
    ("PORTABLE", "포터블"),
    ("INSTALLED", "일반 모드"),
    ("ESC CLOSE", "ESC 닫기"),
    ("FAVORITES", "즐겨찾기"),
    ("RECENT", "최근"),
    ("S  STAR THIS FOLDER", "S  이 폴더 즐겨찾기"),
    ("NO FOLDERS YET", "아직 폴더가 없어요"),
    (
        "ENTER  RENAME      ESC  CANCEL",
        "ENTER  바꾸기      ESC  취소",
    ),
    // canvas and toasts
    ("DROP AN IMAGE", "이미지를 끌어다 놓기"),
    ("DROP TO OPEN", "놓으면 열기"),
    ("OPEN FILE", "파일 열기"),
    ("OPEN FOLDER", "폴더 열기"),
    ("ALL SHORTCUTS", "전체 단축키"),
    ("DECODING …", "읽는 중 …"),
    ("CANNOT OPEN", "열 수 없음"),
    ("DELETE PERMANENTLY?", "완전히 삭제할까요?"),
    (
        "ENTER  DELETE      ESC  CANCEL",
        "ENTER  삭제      ESC  취소",
    ),
    ("LAST IMAGE", "마지막 사진"),
    ("FIRST IMAGE", "첫 사진"),
    ("NO IMAGES IN THIS FOLDER", "이 폴더에 이미지가 없어요"),
    ("MOVED TO RECYCLE BIN", "휴지통으로 이동"),
    ("CTRL+Z UNDO", "CTRL+Z 되돌리기"),
    ("NOTHING TO UNDO", "되돌릴 작업이 없어요"),
    ("UNDO", "되돌림"),
    ("COPY REMOVED", "사본 삭제"),
    ("FILE RESTORED", "파일 복원"),
    ("COPYING …", "복사 중 …"),
    ("COPYING SMALL …", "작게 복사 중 …"),
    ("PATH COPIED", "경로 복사됨"),
    ("EXIF COPIED", "EXIF 복사됨"),
    ("NO EXIF", "EXIF 없음"),
    ("FILE REMOVED", "파일이 지워졌어요"),
    ("NEW IMAGES", "새 이미지"),
    ("NO IMAGE ON THE CLIPBOARD", "클립보드에 이미지가 없어요"),
    ("ZOOM LOCKED", "배율 고정"),
    ("ZOOM UNLOCKED", "배율 고정 해제"),
    ("SMOOTH", "부드럽게"),
    ("PIXEL SCALING", "픽셀 그대로"),
    ("SMOOTH SCALING", "부드럽게 확대"),
    ("ALWAYS ON TOP", "항상 위"),
    ("NORMAL WINDOW", "일반 창"),
    ("SLIDESHOW STOPPED", "슬라이드쇼 정지"),
    ("TOP BAR PINNED", "상단 바 고정"),
    ("TOP BAR AUTO-HIDE", "상단 바 자동 숨김"),
    ("BOTTOM BAR PINNED", "하단 바 고정"),
    ("BOTTOM BAR AUTO-HIDE", "하단 바 자동 숨김"),
    ("DELETED", "삭제됨"),
    ("DELETE FAILED", "삭제 실패"),
    ("COPY FAILED", "복사 실패"),
    ("MOVE FAILED", "이동 실패"),
    ("UNDO FAILED", "되돌리기 실패"),
    ("RENAME FAILED", "이름 바꾸기 실패"),
    ("RENAMED", "이름 바뀜"),
    ("FAILED", "실패"),
    ("DIAGNOSTICS COPIED", "진단 정보 복사됨"),
    ("REGISTERED", "등록됨"),
    (
        "PICK MUUD VIEW IN DEFAULT APPS",
        "기본 앱에서 MUUD VIEW를 고르세요",
    ),
    ("START MENU SHORTCUT ADDED", "시작 메뉴에 추가됨"),
    ("REMOVED FROM WINDOWS", "등록 해제됨"),
    ("KEYMAP EXPORTED", "단축키 내보냄"),
    ("KEYMAP IMPORTED", "단축키 가져옴"),
    ("KEYS RESET", "단축키 초기화"),
    ("FOLDER STARRED", "즐겨찾기 추가"),
    ("FOLDER UNSTARRED", "즐겨찾기 해제"),
    ("ON-SCREEN INFO OFF", "화면 표시 끄기"),
    ("ON-SCREEN INFO NAME", "화면 표시: 이름"),
    ("ON-SCREEN INFO FULL", "화면 표시: 상세"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_falls_back() {
        set_korean(true);
        assert_eq!(tr("NEXT"), "다음");
        assert_eq!(tr("SOMETHING NEW"), "SOMETHING NEW");
        assert_eq!(
            tr_msg("MOVED TO RECYCLE BIN · CTRL+Z UNDO"),
            "휴지통으로 이동 · CTRL+Z 되돌리기"
        );
        assert_eq!(tr_msg("COPY FAILED: disk full"), "복사 실패: disk full");
        set_korean(false);
        assert_eq!(tr("NEXT"), "NEXT");
        set_korean(true);
    }
}
