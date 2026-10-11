from gridcraft_mcp import GridCraftClient


def run_demo():
    print("🤖 [Python] Подключение к GridCraft...")
    try:
        with GridCraftClient(port=7979) as client:
            print("🤖 [Python] Шаг 1: Новая книга")
            client.new_workbook()

            print("🤖 [Python] Шаг 2: Пишем данные (A1, B1, A2, B2)")
            client.set_cell("A1", "Товар")
            client.set_cell("B1", "Цена")
            client.set_cell("A2", "GridCraft")
            client.set_cell("B2", 5000)

            print("🤖 [Python] Шаг 3: Читаем данные ячейки B2")
            val = client.get_cell("B2")
            print(f"   Данные получены: {val}")

            print("🤖 [Python] Демо завершено!")
    except Exception as e:
        print(f"❌ Ошибка подключения: {e}")
        print("   Убедитесь, что GridCraft запущен с флагом --control 7979")


if __name__ == "__main__":
    run_demo()
