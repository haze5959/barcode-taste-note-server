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

---

### 2. `add_product_with_json` (JSON 파일로 제품 일괄 추가)

**실행 명령어:**
```bash
cargo run add_product_with_json

# 또는 부모 디렉토리에서 실행
cargo run -p batch -- add_product_with_json
```

**JSON 파일 위치:** `batch/data/new_product.json`

**JSON 형식:**
```json
[
  {
    "barcode": "4901777317383",
    "product_name": "Suntory Whisky Kakubin",
    "desc": "A blended Japanese whisky from Suntory.",
    "type": "whisky",
    "image_url": "https://example.com/image.jpg"
  }
]
```

| 필드 | 타입 | 설명 |
|------|------|------|
| `barcode` | `string` | 제품 바코드 (중복 시 스킵) |
| `product_name` | `string` | 제품명 (자동으로 Title Case 및 불필요 패턴 제거 적용) |
| `desc` | `string` | 제품 설명 |
| `type` | `string` | 카테고리 (`whisky`, `wine`, `beer`, `soju`, `sake`, `liqueur`, `spirit`, `cocktail`, `coffee`, `beverage`, 기타) |
| `image_url` | `string?` | 이미지 URL (선택값, 있으면 R2 업로드 및 DB 연결) |

**작동 방식 (`What it does`)**
1. `batch/data/new_product.json` 파일을 읽습니다.
2. 각 항목의 `product_name`을 crawler와 동일한 `clean_product_name` 로직으로 정제 (Title Case, 용량·개수 패턴 제거 등) 합니다.
3. 해당 바코드가 이미 `barcodes` 테이블에 존재하면 **스킵**합니다.
4. 동일한 제품명이 `products` 테이블에 이미 존재하면 바코드만 해당 제품에 연결합니다.
5. 신규 제품이면 `products` 테이블에 추가하고, `image_url`이 있는 경우 이미지를 R2에 업로드하여 `product_images`에도 연결합니다.
6. 마지막으로 `barcodes` 테이블에 바코드를 등록합니다.

**로그 예시:**
```
Suntory Whisky Kakubin 추가 성공 (바코드: 4901777317383)
Heineken Beer 이미 존재 (바코드: 8711000010037)
Johnnie Walker Black Label 이미 존재 (동일 제품명, 바코드만 추가)
Empty Product 추가 실패: 이름이 비어 있음 (원본: ...)
```