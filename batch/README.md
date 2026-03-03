# BarcodeTasteNote Batch Project

이 프로젝트는 `BarcodeTasteNoteServer` 애플리케이션의 주기적인 유지보수 및 백그라운드 데이터 처리 작업을 수행하는 독립된 러스트(Rust) 배치 애플리케이션입니다.

## 지원하는 명령어 (Jobs)

### 1. `clean_image` (쓰레기 이미지 정리)

**실행 명령어:**
```bash
cargo run clean_image

# 또는 부모 디렉토리에서 실행
cargo run -p batch -- clean_image
```

**작동 방식 (`What it does`)**
1. 사용자가 리뷰나 상품 등록을 하려다가 중간에 중단하여 DB(`product_images` 테이블)에 덩그러니 남은 **고아 이미지**들을 모두 탐색합니다.
2. 구체적인 탐색 조건은 **"상품(`product_id`)에도 속하지 않고 노트(`note_id`)에도 속하지 않은 상태로 업로드된 지 1시간 이상 경과한 데이터"**입니다.
3. 발견된 데이터들은 DB 테이블에서 안전하게 일괄 삭제(Delete) 처리됩니다.
4. 이후 서버의 실제 이미지 저장장소(`../static/images/`)에 위치한 원본 파일들을 찾아 삭제하지 않고, 나중에 문제가 생겼을 때를 대비해 `../static/trash/` 폴더로 이동시킵니다.
5. 이때 파일의 이름 뒤에 `.jpeg` 확장자를 명시적으로 붙여 보존성을 높입니다.