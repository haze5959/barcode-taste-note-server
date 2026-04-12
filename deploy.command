#!/bin/bash

# 오류 발생 시 스크립트 중지
set -e

# 프로젝트 루트 디렉토리 결정
PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$PROJECT_ROOT"

# 터미널 외부 실행을 대비한 PATH 보장
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"

if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

echo "======================================"
echo "🚀 Starting Production Build & Deploy "
echo "======================================"

# 1. 빌드 전 최신 코드 가져오기 (필요 시 주석 해제)
# echo "🔄 Pulling latest changes from git..."
# git pull origin main

# 2. 릴리즈 모드 빌드 (워크스페이스 전체: 본 서버, batch, crawler 포함)
echo "📦 Compiling workspace in release mode (This may take a while)..."
cargo build --release --workspace

# 3. 배포용 통일 폴더 생성
DEPLOY_DIR="$PROJECT_ROOT/deploy_bin"
mkdir -p "$DEPLOY_DIR"

echo "📂 Copying compiled binaries to $DEPLOY_DIR..."
# 본 서버, batch, crawler 바이너리 복사 (이름을 더 명시적으로 변경하여 관리)
cp "$PROJECT_ROOT/target/release/barcode_taste_note" "$DEPLOY_DIR/barnote_server"
cp "$PROJECT_ROOT/target/release/batch" "$DEPLOY_DIR/barnote_batch"
cp "$PROJECT_ROOT/target/release/crawler" "$DEPLOY_DIR/barnote_crawler"

# 4. 기존 메인 서버 종료
echo "🛑 Stopping existing server processes..."
if pgrep -f "deploy_bin/barnote_server" > /dev/null; then
    pkill -f "deploy_bin/barnote_server"
    
    # 프로세스가 완전히 종료될 때까지 최대 5초간 대기하며 확인 (포트 충돌 방지)
    MAX_WAIT=5
    COUNT=0
    while pgrep -f "deploy_bin/barnote_server" > /dev/null && [ $COUNT -lt $MAX_WAIT ]; do
        echo "   Waiting for server to stop... ($((COUNT+1))s)"
        sleep 1
        ((COUNT++))
    done

    # 5초 뒤에도 살아있다면 강제 종료 수행
    if pgrep -f "deploy_bin/barnote_server" > /dev/null; then
        echo "   ⚠️ Server still running, forcing kill -9..."
        pkill -9 -f "deploy_bin/barnote_server"
        sleep 1
    fi
    echo "   Existing server stopped."
else
    echo "   No existing server found."
fi

# 5. 서브 시스템 로깅 등을 위한 logs 폴더 생성
LOGS_DIR="$PROJECT_ROOT/logs"
mkdir -p "$LOGS_DIR"

# 6. 메인 서버 재시작 (nohup을 통해 백그라운드 데몬화)
echo "🟢 Starting the new Production server..."
# 포트 해제 여유를 위해 잠시 더 대기
sleep 1
# rotatelogs를 사용하여 매일 자정 기준으로 날짜별 로그 파일 자동 분리 (86400초)
nohup sh -c "$DEPLOY_DIR/barnote_server 2>&1 | /usr/sbin/rotatelogs -l '$LOGS_DIR/server_%Y%m%d.log' 86400" > /dev/null 2>&1 &
SERVER_PID=$!

# 7. 실행 확인 (2초 뒤에 확인)
sleep 2
if ps -p $SERVER_PID > /dev/null; then
    echo ""
    echo "✅ Deployment Successful!"
    echo "   - Server is now running in background (PID: $SERVER_PID)"
    echo "   - Main API Server Logs are being rotated automatically into: $LOGS_DIR/server_YYYYMMDD.log"
else
    echo ""
    echo "❌ Server failed to start! Check logs in: $LOGS_DIR"
    exit 1
fi
echo "======================================"
