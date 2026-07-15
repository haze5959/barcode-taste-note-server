# Barnote Batch Project

이 프로젝트는 `BarnoteServer` 애플리케이션의 주기적인 유지보수 및 백그라운드 데이터 처리 작업을 수행하는 독립된 러스트(Rust) 배치 애플리케이션입니다.

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
4. 이후 서버의 실제 이미지 저장장소(`images/`)에 위치한 원본 파일들을 찾아 삭제하지 않고, 나중에 문제가 생겼을 때를 대비해 `deleted/images/` 폴더로 이동시킵니다.
5. 이때 파일의 이름 뒤에 `.jpeg` 확장자를 명시적으로 붙여 보존성을 높입니다.

---

### 2. `add_product_with_json` (JSON 파일로 제품 일괄 추가)

**실행 명령어:**
```bash
cargo run add_product_with_json
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
| `type` | `string` | 카테고리 (`whisky`, `wine`, `beer`, `soju`, `sake`, `liqueur`, `spirit`, `beverage`, 기타) |
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

---

### 3. `backup_db` (DB 백업 후 rclone으로 원격 업로드)

**실행 명령어:**
```bash
cargo run backup_db

# 또는 부모 디렉토리에서 실행
cargo run -p batch -- backup_db
```

**사전 요구사항:**
- `pg_dump` 설치 (`postgresql-client` 패키지)
- `rclone` 설치 및 remote 설정 완료

**환경변수 (`.env`):**

| 변수명 | 기본값 | 설명 |
|--------|--------|------|
| `DATABASE_URL` | (필수) | 백업할 PostgreSQL 연결 문자열 |
| `RCLONE_REMOTE` | `r2` | rclone remote 이름 |
| `RCLONE_BACKUP_PATH` | `barnote-backup/db` | 업로드할 버킷/경로 |
| `DUMP_DIR` | `/tmp/barnote_backup` | 로컬 임시 덤프 저장 디렉토리 |

**작동 방식 (`What it does`)**
1. `DATABASE_URL`을 파싱하여 DB 접속 정보를 추출합니다.
2. `pg_dump`를 실행하여 `{DUMP_DIR}/barnote_db_{YYYYMMDD_HHMMSS}.sql` 파일로 덤프합니다.
3. `rclone copy` 명령으로 덤프 파일을 `{RCLONE_REMOTE}:{RCLONE_BACKUP_PATH}/` 경로에 업로드합니다.
4. 업로드 완료 후 로컬 임시 덤프 파일을 삭제합니다.

**rclone remote 설정 예시 (Cloudflare R2):**
```bash
rclone config create r2 s3 \
  provider Cloudflare \
  access_key_id <R2_S3_ACCESS_KEY_ID> \
  secret_access_key <R2_S3_ACCESS_KEY_SECRET> \
  endpoint <R2_S3_CLIENT> \
  acl private
```

**로그 예시:**
```
=== DB Backup Job 시작 ===
대상 DB: ogyukwon@172.30.1.21/app
덤프 경로: /tmp/barnote_backup/barnote_db_20260329_220900.sql
pg_dump 실행 중...
pg_dump 완료: /tmp/barnote_backup/barnote_db_20260329_220900.sql
rclone 업로드 중: /tmp/barnote_backup/barnote_db_20260329_220900.sql → r2:barnote-backup/db
rclone 업로드 완료
로컬 덤프 파일 삭제 완료: /tmp/barnote_backup/barnote_db_20260329_220900.sql
=== DB Backup Job 완료 ===
업로드 위치: r2:barnote-backup/db/barnote_db_20260329_220900.sql
```

---

### 4. `backup_image` (R2 이미지 전체 로컬 백업)

**실행 명령어:**
```bash
cargo run backup_image

# 또는 부모 디렉토리에서 실행
cargo run -p batch -- backup_image
```

**작동 방식 (`What it does`)**
1. R2(`barnote` 버킷)의 `images/` 경로에 있는 **모든 파일 목록**을 조회합니다.
2. 로컬 `backup/` 폴더에 각 파일을 순차 다운로드하여 저장합니다. (파일명은 R2와 동일한 UUID 형식, `images/profile/` 같은 하위 경로도 동일 구조로 보존)
3. **실행할 때마다 같은 `backup/` 폴더가 갱신됩니다** — 이미 받아둔 파일은 스킵하고 새로 추가된 파일만 다운로드합니다. 중간에 중단되어도 재실행하면 이어서 백업합니다.
4. 백업 폴더 위치는 **명령어를 실행한 디렉토리 기준 상대 경로**입니다.

**로그 예시:**
```
=== Image Backup Job 시작 ===
백업 위치: backup
총 4230개 파일 백업 시작
진행 100/4230 (다운로드 100, 스킵 0, 실패 0)
...
=== Image Backup Job 완료 ===
백업 위치: backup (다운로드 130, 스킵 4100, 실패 0)
```

---

## 📅 일일 자동화 작업 (Scheduled Jobs)

매일 정기적으로 수행해야 하는 작업들(`backup_db`, `backup_image` 등)을 자동화하기 위해 크론탭(crontab)을 사용할 수 있습니다.

### 1. 전용 스케줄링 스크립트
작업의 순차 실행 및 실패 시 알림을 위해 전용 스크립트(프로젝트 루트의 `daily_job.sh`)를 제공합니다.

- **위치:** `daily_job.sh` (프로젝트 루트)
- **담당 작업:** (배포 바이너리 `deploy_bin/barnote_batch` 사용)
    1. `barnote_batch backup_db` 실행 (DB 백업 후 원격 업로드)
    2. `barnote_batch backup_image` 실행 (R2 이미지 → 로컬 `backup/` 폴더 갱신)
    3. 크롤러 실행은 현재 스크립트에서 주석 처리되어 있습니다 (필요 시 해제)
    4. **알림:** 작업 중 하나라도 실패(Exit Code != 0)하면 `barcodetastenote@gmail.com`으로 실패 레포트를 메일로 발송합니다.
- **로그 저장:** 작업 상세 내역은 `deploy_bin/daily_job.log` 파일에 기록됩니다.

### 2. 크론탭(Crontab) 설정
매일 **오전 7시**에 해당 스크립트가 실행되도록 설정하는 방법입니다.

터미널에서 `crontab -e`를 입력하고 아래 내용을 추가하십시오:

```bash
# 매일 오전 7시 00분에 Barnote 일일 통합 작업 실행
0 7 * * * /Users/ogyukwon/Documents/Projects/BarcodeTasteNoteServer/daily_job.sh
```

> **주의:** 스크립트 경로는 실제 환경의 절대 경로로 수정하여 사용해야 합니다.
> 시스템에 `mail` 명령어 및 메일 서버(Postfix 등)가 사전 설정되어 있어야 메일 발송 기능이 정상 작동합니다.