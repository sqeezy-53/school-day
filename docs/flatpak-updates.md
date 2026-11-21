# Обновления Flatpak

Релизная сборка создаёт локальный OSTree в каталоге сборки. Для публикации владелец выбирает HTTPS-хостинг статических файлов и постоянный ключ подписи. Flathub может стать отдельным каналом после принятия приложения. В этой конфигурации используется собственный репозиторий.

Скопируйте предыдущее дерево публикации в staging. Для первого выпуска используйте пустой staging. После успешной сборки выполните:

```sh
stage=$(cat "$BUILD_WORK/latest-build.txt")
python3 packaging/flatpak/prepare-repository.py --source "$stage/repo" \
  --destination "$STAGING" --url "$PUBLIC_REPO_URL" \
  --keyring "$GNUPGHOME" --fingerprint "$SIGNING_FINGERPRINT"
```

В GitHub Actions исходный OSTree доступен в артефакте school-day-ostree. Распакуйте TAR и передайте каталог repo в --source.

Выложите содержимое staging целиком, включая summary, подписи и старые объекты. Переключите опубликованное дерево атомарно после проверки. Храните секретный ключ отдельно. Сверьте отпечаток открытого ключа через доверенный канал перед первой установкой.

Новая установка выполняется из опубликованного school-day.flatpakref. Центр приложений или команда flatpak update проверяют обновления в этом источнике. Автоматическая установка зависит от настроек системы.

## Переход с одиночного файла

Создайте копию расписания. В старой версии с закрытым приложением скопируйте каталог ~/.var/app/io.github.sqeezy_53.school-day/data/school-day целиком. Узнайте, куда установлено приложение, командами ниже:

```sh
flatpak list --app --columns=application,installation,origin,branch
flatpak remote-add --user --if-not-exists school-day ./school-day.flatpakrepo
flatpak install --user --reinstall school-day io.github.sqeezy_53.school-day//master
flatpak info --user --show-origin io.github.sqeezy_53.school-day
flatpak update --user io.github.sqeezy_53.school-day
```

Для существующей системной установки используйте --system во всех четырёх командах вместо --user. Сохранены идентификатор приложения и ветка master. Проверьте источник school-day и откройте расписание. Для переноса на Android используйте экспорт расписания в настройках. Для отката на 0.6.2 закройте приложение и восстановите исходную резервную копию lessons.json.
