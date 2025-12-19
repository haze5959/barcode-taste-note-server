#!/bin/bash
set -euo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

# ====== 설정 ======
# Codex CLI 커맨드 (환경에 맞게 바꿔 쓰거나 env로 덮어쓰기)
CODEX_CMD="${CODEX_CMD:-codex}"

# 템플릿 파일 경로 (기본: 현재 폴더의 template.prompt.md)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATE_FILE_DEFAULT="$SCRIPT_DIR/template.prompt.md"
TEMPLATE_FILE="${TEMPLATE_FILE:-$TEMPLATE_FILE_DEFAULT}"

# ====== codex 존재 확인 ======
if ! command -v "$CODEX_CMD" >/dev/null 2>&1; then
  echo "❌ Codex CLI를 찾을 수 없음: $CODEX_CMD"
  echo "PATH: $PATH"
  echo
  echo "해결:"
  echo "1) 터미널에서 'which codex'로 위치 확인"
  echo "2) CODEX_CMD나 PATH를 맞춰주세요."
  exit 1
fi

# ====== 템플릿 로드 ======
if [[ ! -f "$TEMPLATE_FILE" ]]; then
  echo "❌ 템플릿 파일을 찾을 수 없음: $TEMPLATE_FILE"
  exit 1
fi

TEMPLATE="$(cat "$TEMPLATE_FILE")"

# ====== 입력 받기 ======
# 인자 Goal이 주면 그걸 쓰고, 아니면 인터랙티브 입력
if [[ $# -ge 1 ]]; then
  GOAL="$1"
else
  echo "Goal을 입력하세요. (여러 줄 가능 / 끝내려면 Ctrl-D)"
  GOAL="$(cat)"
fi

# ====== 치환 ======
# bash의 문자열 치환을 써서 {{GOAL}}, {{SCOPE}}를 바꾼다
PROMPT="${TEMPLATE//\{\{GOAL\}\}/$GOAL}"

# ====== 최종 프롬프트 출력(확인용) ======
echo
echo "================= Final Prompt ================="
echo "$PROMPT"
echo "================================================"
echo

# ====== Codex로 전달 ======
cd "$SCRIPT_DIR"
if "$CODEX_CMD" exec --sandbox danger-full-access "$PROMPT"; then
  "$CODEX_CMD" resume --sandbox danger-full-access
else
  "$CODEX_CMD" --sandbox danger-full-access "$PROMPT"
fi