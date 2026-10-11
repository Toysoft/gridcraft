#!/bin/bash

# Скрипт для настройки Git, коммита и пуша изменений в форк
# Убедитесь, что вы сделали Fork репозитория storytold/gridcraft на свой аккаунт GitHub!

REMOTE_FORK="git@github.com:dronov-dmitry/gridcraft.git"
BRANCH_NAME="feat-python-mcp-client"

echo ">>> Настройка Git репозитория..."

# Инициализация, если не git
if [ ! -d ".git" ]; then
    git init
    echo "Initialized empty git repository."
fi

# Установка remote для вашего форка
if ! git remote get-url origin &>/dev/null; then
    git remote add origin $REMOTE_FORK
    echo "Added origin: $REMOTE_FORK"
else
    echo "Remote 'origin' already exists."
fi

# Настройка ветки
if git show-ref --verify --quiet refs/heads/$BRANCH_NAME; then
    git checkout $BRANCH_NAME
else
    git checkout -b $BRANCH_NAME
fi

# Добавление всех файлов
git add .

# Коммит
if git commit -m "feat(python): add Python MCP client library for GridCraft automation"; then
    # Пуш в ваш форк
    echo ">>> Отправка изменений в ваш форк..."
    git push -u origin $BRANCH_NAME
    echo ">>> Готово! Теперь перейдите по ссылке, которую выдал git, чтобы создать Pull Request."
else
    echo ">>> Нет изменений для коммита или ошибка коммита."
fi
