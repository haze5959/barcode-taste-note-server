#!/bin/bash

# 프로젝트 루트 디렉토리로 이동 (스크립트 위치 기준: batch/scripts/daily_job.sh -> 루트)
PROJECT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$PROJECT_ROOT"

# 크론탭(Cron)은 기본 PATH가 매우 제한적이므로, Cargo 및 시스템 바이너리 경로를 명시해 줍니다.
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"

# Rust 환경 변수 로드 (설치 방식에 따라 필요할 수 있음)
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

LOG_FILE="$PROJECT_ROOT/batch/daily_job.log"
EMAIL="barcodetastenote@gmail.com"

echo "[$(date)] === Daily Job Started ===" >> "$LOG_FILE"

# 1. Crawler 실행
echo "[$(date)] Running Crawler..." >> "$LOG_FILE"
cargo run -p crawler >> "$LOG_FILE" 2>&1
CRAWLER_STATUS=$?

# 2. DB Backup 실행
echo "[$(date)] Running DB Backup..." >> "$LOG_FILE"
cargo run -p batch -- backup_db >> "$LOG_FILE" 2>&1
BACKUP_STATUS=$?

# 실패 시 메일 발송
if [ $CRAWLER_STATUS -ne 0 ] || [ $BACKUP_STATUS -ne 0 ]; then
    echo "[$(date)] Job Failed. Sending email to $EMAIL..." >> "$LOG_FILE"
    
    SUBJECT="[Barnote] Daily Job Failure Report ($(date +%Y-%m-%d))"
    MESSAGE="Barnote daily automated tasks failed.\n\n"
    MESSAGE+="Date: $(date)\n"
    MESSAGE+="Crawler Status: $CRAWLER_STATUS (0 is success)\n"
    MESSAGE+="Backup Status: $BACKUP_STATUS (0 is success)\n\n"
    MESSAGE+="Please check the log file at: $LOG_FILE"
    
    # mail 명령어를 사용하여 발송 (시스템에 postfix/sendmail 등이 설정되어 있어야 함)
    echo -e "$MESSAGE" | mail -s "$SUBJECT" "$EMAIL"
else
    echo "[$(date)] All jobs completed successfully." >> "$LOG_FILE"
fi

echo "[$(date)] === Daily Job Finished ===" >> "$LOG_FILE"
echo "" >> "$LOG_FILE"
