# Barcode Taste Note Server

이 문서에서는 서버 배포 스크립트 및 유지보수를 위한 자동화 스크립트, 그리고 배치 작업(Batch Jobs) 명령어에 대해 설명합니다.

---

## 🚀 배포 및 자동화 스크립트

### 1. `deploy.command` (운영 배포 스크립트)
프로젝트의 전체 소스코드를 프로덕션(Production) 모드로 빌드하고 메인 API 서버를 백그라운드에서 구동하는 단일 스크립트입니다.

- **주요 동작**:
    1. `cargo build --release --workspace`를 실행하여 본 서버, 배치(batch), 크롤러(crawler)를 모두 최적화 컴파일합니다.
    2. 생성된 실행 파일들을 `deploy_bin` 폴더로 복사하고, 각각 `barnote_server`, `barnote_batch`, `barnote_crawler`로 직관적인 이름으로 변경합니다.
    3. 구동 중인 기존 서버를 무중단에 가깝게 찾아 강제 종료합니다.
    4. 새로 빌드된 `barnote_server`를 `nohup`을 통해 백그라운드 데몬으로 실행시킵니다.
    5. 서버 로그는 `logs/server_YYYYMMDD.log` 형태로 저장됩니다.

### 2. `daily_job.sh` (일일 정기 자동화 스크립트)
서비스에 최신 상품을 공급하고, DB 안정성을 보장하기 위해 매일 정해진 시간(주로 크론탭을 통해 오전 7시)에 백그라운드에서 실행되는 자동화 셸 스크립트입니다.

- **주요 동작**:
    1. **크롤러 실행**: 빌드 완료된 `deploy_bin/barnote_crawler`를 실행하여 새로운 상품 정보를 가져옵니다.
    2. **DB 백업 실행**: 빌드 완료된 `deploy_bin/barnote_batch backup_db`를 실행하여 데이터베이스 스냅샷을 생성하고 외부 스토리지(R2)에 보관합니다.
    3. **에러 핸들링**: 두 작업 중 하나라도 실패(Exit Code가 0이 아님)할 경우 지정된 유지보수 이메일(`barcodetastenote@gmail.com`)로 Postfix를 통해 장애 알림을 전송합니다.
    4. 모든 로그는 `batch/daily_job.log` 파일에 영구 기록됩니다.

---

## 📦 배치 모듈 (`deploy_bin/barnote_batch`)

배치(Batch) 프로젝트는 일회성 혹은 정기적 유지보수를 위한 모음집입니다. 다음 명령어를 통해 실행할 수 있습니다:

```bash
./deploy_bin/barnote_batch <명령어>
```

### 1. `clean_image` 명령어
- **설명**: 불필요한 이미지 데이터를 정리하여 클라우드 스토리지 요금 폭탄을 방지하고 DB 무결성을 유지하는 캐시 정리 도구입니다.
- **상세**:
    - DB 내의 `product_images` 테이블에는 존재하지만 실제 노트(note)나 상품(product)에 연결되지 않은 찌거기(고아) 이미지 레코드를 찾아 DB에서 삭제합니다.
    - 동시에 연결된 실제 이미지 파일을 Cloudflare R2 버킷에서 물리적으로 삭제합니다.

### 2. `add_product_with_json` 명령어
- **설명**: 외부 혹은 관리자가 수집한 대용량의 JSON 포맷 상품 데이터를 시스템에 한 번에 밀어 넣기 위한 마이그레이션 도구입니다.
- **상세**:
    - 사전에 정의된 JSON 파일의 배열 구조를 순회하면서 파싱합니다.
    - `products` 테이블에 새로운 상품 정보를 삽입하고, 해당 상품과 연동된 바코드 정보를 `barcodes` 테이블에 트랜잭션으로 안전하게 저장합니다.

### 3. `backup_db` 명령어
- **설명**: 데이터 유실을 대비해 PostgreSQL 데이터베이스의 전체 상태를 안전하게 외부에 보관하는 백업 도구입니다. (`daily_job.sh`에서 호출됨)
- **상세**:
    - 로컬 시스템에 `pg_dump`를 실행하여 그날의 날짜와 시간이 적힌 덤프 파일(`.sql`)을 생성합니다.
    - `rclone` 도구를 이용해 지정된 Cloudflare R2 버킷(`barnote-backup/db`)으로 안전하게 업로드합니다.
    - 용량 누수 방지를 위해 R2 저장소에서 보관 기간이 **7일을 초과한 과거 백업 파일들을 자동으로 삭제**해 줍니다.

---

## 🛑 서버 제어 가이드

### 1. 서버 프로세스 수동 종료 (Stop)
현재 백그라운드에서 실행 중인 메인 API 서버(`barnote_server`)를 강제로 끄려면 터미널에 다음 명령어를 입력하세요.
```bash
pkill -f "deploy_bin/barnote_server"
```
명령어를 치고 아무 메시지가 뜨지 않았다면 정상적으로 종료된 것입니다.

### 2. 서버 프로세스 수동 재시작 (Restart)
수정된 코드를 **새로 반영하여 재시작하고 싶을 때**는 프로젝트 최상단의 **`deploy.command`를 실행**하는 것이 가장 안전하고 빠릅니다. 

하지만, (재빌드 없이) **현재 컴파일된 버전 그대로 서버만 껐다가 켜고 싶을 때**는 프로젝트 루트 디렉토리에서 다음 2줄의 명령어를 차례로 입력하세요.

```bash
# 1️⃣ 기존에 돌고 있는 서버를 먼저 종료합니다.
pkill -f "deploy_bin/barnote_server"

# 2️⃣ 서버를 백그라운드(nohup)로 다시 실행시킵니다. (로그는 logs/ 폴더에 기록)
nohup ./deploy_bin/barnote_server > "logs/server_$(date +%Y%m%d).log" 2>&1 &
```
