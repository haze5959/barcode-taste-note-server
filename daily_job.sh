#!/bin/bash

# 프로젝트 루트 디렉토리로 이동 (daily_job.sh 스크립트 위치 기준)
PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$PROJECT_ROOT"

if [ -f "$PROJECT_ROOT/.env" ]; then
    set -a
    source "$PROJECT_ROOT/.env"
    set +a
fi

# 크론탭 환경에서는 Homebrew PATH가 빠져 있으므로 명시적으로 추가합니다.
# - /opt/homebrew/opt/libpq/bin : pg_dump 경로
# - /opt/homebrew/bin           : rclone 등 일반 Homebrew 도구 경로
export PATH="/opt/homebrew/opt/libpq/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH"


LOG_FILE="$PROJECT_ROOT/deploy_bin/daily_job.log"
EMAIL="barcodetastenote@gmail.com"

echo "#########[$(date)]############" >> "$LOG_FILE"

# 1. Crawler 실행
# echo "Running Crawler..." >> "$LOG_FILE"
# $PROJECT_ROOT/deploy_bin/barnote_crawler >> "$LOG_FILE" 2>&1
# CRAWLER_STATUS=$?

# 2. DB Backup 실행
echo "Running DB Backup..." >> "$LOG_FILE"
$PROJECT_ROOT/deploy_bin/barnote_batch backup_db >> "$LOG_FILE" 2>&1
BACKUP_STATUS=$?

# 실패 시 메일 발송
if [ $CRAWLER_STATUS -ne 0 ] || [ $BACKUP_STATUS -ne 0 ]; then
    echo "Job Failed. Sending email to $EMAIL..." >> "$LOG_FILE"
    
    SUBJECT="[Barnote] Daily Job Failure Report ($(date +%Y-%m-%d))"
    MESSAGE="Barnote daily automated tasks failed.\n\n"
    MESSAGE+="Date: $(date)\n"
    MESSAGE+="Crawler Status: $CRAWLER_STATUS (0 is success)\n"
    MESSAGE+="Backup Status: $BACKUP_STATUS (0 is success)\n\n"
    MESSAGE+="Please check the log file at: $LOG_FILE"
    
    # mail 명령어를 사용하여 발송 (시스템에 postfix/sendmail 등이 설정되어 있어야 함)
    echo -e "$MESSAGE" | mail -s "$SUBJECT" "$EMAIL"
else
    echo "All jobs completed successfully." >> "$LOG_FILE"
fi
echo "###################################################" >> "$LOG_FILE"
echo "" >> "$LOG_FILE"