#!/bin/bash

# Visual Demo Script for GridCraft MCP Python Client

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() { echo -e "${CYAN}>>> ${1}${NC}"; }
warn() { echo -e "${YELLOW}⚠ ${1}${NC}"; }

# Проверка, запущен ли GridCraft
if ! ss -tuln 2>/dev/null | grep -q ":7979 "; then
    warn "GridCraft не обнаружен на порту 7979."
    echo ""
    log "Пожалуйста, запустите GridCraft в другом терминале:"
    echo -e "${CYAN}    gridcraft --control 7979${NC}"
    echo ""
    read -p "Нажмите Enter, когда приложение будет запущено..."
fi

# Определяем абсолютный путь к папке скриптов
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Установка пути к модулю (папка libs/mcp-python)
export PYTHONPATH="$SCRIPT_DIR/.."

log "Запуск Python-демонстрации..."
python3 "$SCRIPT_DIR/demo_visual.py"
