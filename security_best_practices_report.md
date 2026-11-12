# Проверка безопасности OctoFHIR

Дата: 12 ноября 2026. Проверенная ревизия: `7a154c59a5560fea2dfc4ac788cf243916a25e0a`.

## Результат

Найдены практически значимые проблемы: обход ограничений SQL-консоли, SSRF с передачей
учётных данных из серверных блокнотов, небезопасный HTML и выполнение JavaScript в UI
блокнотов, неполное применение исходящей сетевой политики. Приоритет — закрыть эти
границы до предоставления доступа недоверенным пользователям.

**SQL → выполнение команды ОС подтверждено на отдельной временной PostgreSQL**, с правами
суперпользователя и SQL, который пропускает режим `readwrite`. Это выполнение на машине
PostgreSQL, не доказательство выполнения в процессе Rust-сервера. Неаутентифицированный
RCE в текущем HTTP-сервере не подтверждён.

Зависимости проверены по актуальным базам. Rust: одна CVE и три предупреждения.
npm: 87 package-level alerts по счётчику npm, включая два Critical; это общая
монорепозитория с документацией и инструментами разработки. Локальный Docker-образ
проверен отдельно и не отождествляется с текущим исходным кодом.

## Объём и достоверность

- Изучены Rust/Axum middleware, SQL-консоль, gateway, серверные блокноты,
  QuickJS-политики, извлечение пакетов, исходящие HTTP-запросы, React UI,
  конфигурации, Dockerfile и CI.
- Проверены корневой `Cargo.lock`, `tests/library-consumer/Cargo.lock` и корневой
  `pnpm-lock.yaml`. Docker использует этот корневой pnpm lockfile.
- Использованы `cargo-audit 0.22.1`, `pnpm 10.11.0`, `Trivy 0.74.0`.
  RustSec: 1490 advisories, commit `af7b3ec33460b8e2de8d422e16d628518891a519`,
  обновлён 2026-11-12 15:21:59 +01:00.
- Проверки SQL использовали извлечённую из текущего исходника реализацию фильтра и
  новую PostgreSQL с Unix-сокетом и пустой БД. Кластер удалён после проверки.
- Сетевые проверки использовали настоящую `run_cell` и `EgressPolicy`, извлечённые
  из текущих файлов, два слушателя на `127.0.0.1` и фиктивный токен.
- UI-проверки исполняли текущий TS/TSX через TypeScript и Node; HTML проверен
  React server rendering. Автоматизированная проверка исполнения XSS в браузере
  и полный HTTP-сценарий с OAuth/политиками не выполнялись.
- Рабочая БД и удалённое развёртывание не тестировались. Отчёт не устанавливает,
  какие настройки и права используются в production. Fuzzing и полный аудит
  исходников всех транзитивных зависимостей в эту проверку не входят.
- Production-код и lockfiles не изменены. Добавлены отчёт, результаты сканеров и
  воспроизводимые проверки. Рекомендации навыка security-best-practices применены
  к React; отдельных Rust-рекомендаций в нём нет.

## Приоритеты

| ID | Уровень | Проблема | Проверка |
|---|---|---|---|
| SEC-01 | High; Critical при доступе к SQL с правами ОС-команд | SQL `readonly`/`readwrite` обходятся | Реальный фильтр + временная PostgreSQL |
| SEC-02 | High | Серверный Notebook: SSRF и утечка Authorization | Реальная функция + локальный HTTP-получатель |
| SEC-03 | High | Сохранённый HTML блокнота попадает в DOM без очистки | Прослежен путь данных, проверен HTML компонента |
| SEC-04 | High с запуском ячейки пользователем | Pipeline исполняет произвольный JS и интерполированные данные | Исполнение текущего кода в Node |
| SEC-05 | High | Часть исходящих запросов обходит даже строгую egress-политику | Реальная политика + локальный HTTP-получатель |
| SEC-06 | High при публикации стандартной конфигурации | Известные bootstrap-пароли и SQL без обязательной роли | Исходный код и конфигурации |
| SEC-07 | Medium, условная достижимость | Уязвимый SQLite в Rust-зависимостях | RustSec + дерево и features зависимостей |
| SEC-08 | Critical upstream; применимость зависит от компонента | Astro/Storybook и прочие npm advisories | npm audit + advisories разработчиков |
| SEC-09 | Требует отдельной оценки достижимости | CVE и устаревшие пакеты локального образа | Trivy, конкретный image ID |

## High / условный Critical

### SEC-01. SQL-режимы не обеспечивают заявленные ограничения

**Влияние:** имеющий доступ к консоли пользователь может выполнять изменения в режиме
`readonly`; при `readwrite` и правах `superuser`/`pg_execute_server_program` — запускать
команды ОС PostgreSQL.

Код:

- `crates/octofhir-server/src/config.rs:1041` — `is_query_allowed`, проверка префикса
  и blacklist подстрок; ветка `readwrite` — строка 1076.
- `crates/octofhir-server/src/operations/sql.rs:133` — необязательная дополнительная
  роль; строки 181–192 — выполнение пользовательского SQL через общий `state.db_pool`.
- `crates/octofhir-server/src/gateway/sql.rs:112` — похожий фильтр SQL gateway;
  выполнение через общий pool — строка 64. Здесь SQL задаётся ресурсом операции,
  а не автоматически берётся из параметров её вызывающего.

Фильтр `readonly` принимает одиночные запросы:

```sql
SELECT 42 AS marker INTO audit_probe;
SELECT lo_from_bytea(0, decode('61', 'hex'));
SELECT pg_read_file('/tmp/audit-only-marker');
SELECT pg_sleep(1);
```

Первый запрос создаёт таблицу. Второй создаёт large object. Третий требует соответствующих
прав PostgreSQL и показывает, почему разрешение произвольных SELECT не изолирует файлы.
Временная PostgreSQL подтвердила создание таблицы и large object.

Фильтр `readwrite` принимает:

```sql
COPY (SELECT 'audit-marker') TO PROGRAM 'true';
```

Во временной БД с суперпользователем результат — `COPY 1`: безвредная команда `true`
выполнена. Многооператорный SQL и обход prepared statements для этого не нужны.
PostgreSQL документирует выполнение `PROGRAM` через shell и необходимые права:
[COPY](https://www.postgresql.org/docs/current/sql-copy.html).

Условия: запрос должен пройти аутентификацию и AccessPolicy для `/api/$sql`, консоль
должна быть включена, а `required_role`, если задана, должна совпасть. Поэтому это
не утверждение о доступности SQL любому пользователю или без токена. Режим `admin`
преднамеренно разрешает SQL, но требует особенно узкого доступа и безопасной роли БД.

Исправление:

1. Отдельная роль/отдельный pool для SQL-консоли; без superuser,
   `pg_execute_server_program`, `pg_read_server_files`, `pg_write_server_files` и
   доступных привилегированных функций. Разделить роль миграций и runtime.
2. В `readonly` выполнять запрос в транзакции `READ ONLY`; дополнить PostgreSQL-aware
   разбором одного разрешённого statement, запретом `SELECT INTO` и контролем функций.
   Одного `READ ONLY` недостаточно против привилегированного чтения файлов и всех
   внешних побочных эффектов. См. [SET TRANSACTION](https://www.postgresql.org/docs/current/sql-set-transaction.html).
3. В `readwrite` разрешать конкретные типы DML через AST, а не всё, что не попало в
   blacklist. Не давать пользовательскому SQL административные возможности pool.
4. Сделать административную роль обязательной для консоли; для клинических запросов
   использовать API с проверкой SMART scopes/compartments вместо прямого SQL.
5. Ввести DB `statement_timeout`, `lock_timeout`, лимит строк/размера ответа и
   ограничения параллелизма. Сейчас обработчик использует `fetch_all`, не задаёт этих
   ограничений; HTTP timeout сам по себе не является достаточной защитой БД и памяти.

Проверки: [sql_probe.py](reports/security-2026-11-12/sql_probe.py),
[результаты](reports/security-2026-11-12/sql-postgres-results.txt).

### SEC-02. Серверный запуск блокнота позволяет SSRF и пересылает Authorization

Код: `crates/octofhir-server/src/notebook_run.rs:414` принимает абсолютный URL ячейки;
строка 422 создаёт запрос к нему. Строки 505–515 устанавливают входящий `Authorization`
как default header общего reqwest-клиента, предназначенного также для этих URL.

Путь: `POST /api/notebooks/{id}/run` → сохранённый `Notebook` → REST-ячейка →
произвольный `http(s)` URL. Клиент не использует `EgressPolicy`; настройка
`server.allow_private_outbound = false` этот путь не закрывает. Если пользователь
запускает документ с чужой REST-ячейкой, его Bearer или Basic Authorization может
уйти владельцу указанного адреса. Содержимое ответа возвращается как output ячейки.

Локальная проверка настоящей функции `run_cell` с клиентом, настроенным как в обработчике,
подтвердила запрос к `127.0.0.1` и получение `Bearer AUDIT-DUMMY-NOT-A-REAL-TOKEN`
сервером-получателем. Добавленные в стенде timeout и `no_proxy` только ограничивают
тест; они не добавляют проверку назначения или разделение учётных данных.

Условия: доступ через общую AccessPolicy к endpoint запуска и наличие доступного
сохранённого документа. Для кражи чужих учётных данных жертва должна запустить
контролируемый атакующим документ с Authorization header. Cookie исходного запроса
этот обработчик не копирует; это не опровергает SSRF, но ограничивает сценарий утечки.

Исправление: разделить клиент внутренних вызовов и внешний HTTP-клиент. Передавать
Authorization только явно разрешённому внутреннему origin. Для внешних URL применять
egress-проверку IP и DNS при соединении, проверку схемы и запрет/проверку redirect,
таймауты и размер ответа. Ещё безопаснее разрешать в headless REST только внутренние
FHIR-пути. Проверять право чтения конкретного Notebook до запуска, а не только право
вызова API: сейчас handler использует `read_raw` напрямую на строке 496.

Проверки: [network_probe.py](reports/security-2026-11-12/network_probe.py),
[результаты](reports/security-2026-11-12/network-results.txt).

### SEC-03. HTML output блокнота не очищается перед вставкой в DOM

Код: `ui/src/pages/notebooks/components/CellOutput.tsx:75` передаёт `output.html`
в `dangerouslySetInnerHTML`. Комментарий «sanitized upstream» не подкреплён очисткой
на проверенном пути загрузки:

`api/notebookApi.ts:34` → `readNotebook` → `NotebookEditorPage.tsx:55` →
`model/store.ts:69` → `CellFrame.tsx:197` → `CellOutput`.

Модель разрешает `outputs` и `kind: html`; native `.fhirnb` импорт использует обычный
serde parse (`crates/octofhir-notebook/src/lib.rs:67`, строка 86). Сохранённые outputs
не становятся доверенными из-за TypeScript-типа. Очистка outputs при сохранении в UI
не защищает загрузку чужого документа и не является серверной границей.

Текущий компонент сохраняет в результирующем HTML активный обработчик:

```html
<img src="audit-missing" onerror="globalThis.__octofhirAuditMarker=7">
```

Условия: недоверенный Notebook с таким output должен быть принят хранилищем/импортом
и открыт жертвой с отображением outputs. При строгой структурной валидации конкретной
инсталляции принятие такого документа нужно проверить отдельно. Защитная CSP от
reverse proxy также может уменьшать последствия; она в этой проверке не известна.

Влияние — JavaScript в origin административного UI. `ui/src/shared/api/authSession.ts:53`
хранит refresh token в sessionStorage, поэтому возможна не только отправка запросов от
имени пользователя, но и кража доступного JavaScript токена.

Исправление: удалить поддержку raw HTML либо очищать его поддерживаемым HTML sanitizer
непосредственно перед sink. При необходимости интерактивного HTML использовать
изолированный sandboxed iframe с отдельным origin и без доступа к сессии. Удалять
недоверенные кешированные outputs при импорте/загрузке. Добавить браузерный regression
test с event handlers и опасными URL. Проверенный экспорт HTML на сервере, напротив,
экранирует содержимое — эта находка относится к UI.

Проверка: [ui_probe.cjs](reports/security-2026-11-12/ui_probe.cjs),
[результаты](reports/security-2026-11-12/ui-results.txt).

### SEC-04. Pipeline использует JS как вычислитель выражений в origin UI

Код: `ui/src/pages/notebooks/model/pipeline.ts:27` интерполирует scope, затем строка 30
создаёт `new Function`. `model/execution.ts:261` вызывает это при запуске pipeline.
Функция имеет доступ к `globalThis`, DOM, sessionStorage и fetch в браузере.

Проверены два случая на текущем коде:

- выражение `(globalThis.__octofhirAuditMarker = 7)` исполняется;
- обычный шаблон `"${value}"` превращается в код, если значение переменной содержит
  `x" + (globalThis.__octofhirAuditMarker = 9) + "`.

Это не серверный RCE. Атака требует запуска ячейки/Run All с недоверенным выражением
или данными, интерполируемыми в выражение. Функция вычисления формул не должна
неявно давать импортированному документу полномочия административного UI.

Исправление: ограниченный parser/interpreter для выражений, переменные передавать как
данные; не вставлять значения в исходный JS. Если произвольный JavaScript нужен как
функция продукта, исполнять его в отдельном origin с ограниченным протоколом обмена
данными и явной моделью доверия. Один Web Worker не изолирует same-origin сеть.

### SEC-05. Egress-защита не применяется ко всем путям отправки

Код:

- `crates/octofhir-server/src/egress.rs:187` устанавливает DNS resolver, но сам
  reqwest-клиент не вызывает `check_url` для IP literals.
- `server.rs:1978` создаёт такой клиент для AppSubscription;
  `hooks/app_subscription.rs:92` отправляет webhook без `check_url`.
- `server.rs:281` передаёт похожий клиент процессору уведомлений;
  `crates/octofhir-notifications/src/adapters/webhook.rs:105` начинает POST к выбранному
  URL без эквивалентной проверки IP literal.
- `gateway/router.rs:35` создаёт обычный клиент для proxy, а
  `gateway/proxy.rs:111` использует его для заданного операцией URL без egress-защиты.

Локальный тест подтвердил: `EgressPolicy::new(false, &[])` отклоняет loopback через
`check_url`, однако клиент из `client_builder` успешно делает POST к тому же адресу.
Следовательно, отсутствие дополнительной проверки у вызывающего кода — реальный обход.
Default redirect policy в этих клиентах также допускает переход на IP literal.

Условия: возможность задавать/менять соответствующий webhook, Notification либо proxy
operation и инициировать отправку. Права на эти ресурсы зависят от AccessPolicy.
Риск proxy ниже, если App/CustomOperation управляются только доверенными администраторами.

Исправление: один общий API отправки, который проверяет исходный URL и каждый redirect,
а DNS проверяет при соединении. Перевести на него все webhooks, notifications, proxy
и headless REST. Пример более полного применения уже есть в
`subscriptions/delivery/rest_hook.rs:29` и строке 132: проверка URL + resolver +
отключённые redirects. Добавить tests для IP literal, redirect на private IP и DNS rebinding.

Дополнительно: `config.rs:289` по умолчанию разрешает private outbound. В production
надо явно отключить его и задать минимальный allowlist; это не устраняет перечисленные
пути обхода само по себе.

### SEC-06. Стандартные bootstrap-учётные данные и опасные настройки консоли

`Dockerfile:231` создаёт bootstrap admin с паролем `admin`.
`octofhir.toml:399` содержит известный dev-пароль, а строка 404 — известный backend
client secret. Там же, `octofhir.toml:367`, SQL включён в `admin`-режиме без обязательной
роли. Docker включает консоль в `readonly`, также без `required_role`.
Defaults `DbConsoleConfig` включают консоль и `required_role: None`
(`config.rs:1016`). У общей AccessPolicy остаётся право отклонить запрос.

Это конфигурационный риск при публикации конфигурации по умолчанию; действующие пароли
production не проверялись. README/SECURITY.md предупреждают о dev-конфигурации, но
публичный runtime-образ также создаёт известную административную учётную запись.

Исправление: убрать пароль из runtime-образа, требовать secret при bootstrap либо
создавать одноразовый случайный пароль; закрыть консоль по умолчанию или потребовать
admin-роль. Запретить известные bootstrap-пароли при запуске production-профиля.
Runtime-роли PostgreSQL не должны быть `postgres`/superuser.

Важно: `bootstrap.rs:487` пропускает уже существующего пользователя. Изменение env
bootstrap password само по себе не меняет пароль созданного admin — нужна реальная
ротация пользователя и backend client secret в хранилище.

## Зависимости и образ

### SEC-07. Уязвимый SQLite и предупреждения RustSec

| Зависимость | Advisory | Установлено | Исправление | Применимость |
|---|---|---|---|---|
| libsqlite3-sys | RUSTSEC-2026-0459 / CVE-2026-119375 | 0.38.2 | >=0.39.0 | Локальный crafted DB + WAL, утечка памяти |
| lru | RUSTSEC-2026-0253, INFO Unsound | 0.16.4 | >=0.18.2 | Требует panic в Drop ключа, unwind и продолжения после panic |
| astral-tokio-tar | RUSTSEC-2026-0440 | 0.6.4 | Замена upstream | Unmaintained; приходит через dev-only testcontainers |
| bincode | RUSTSEC-2025-0141 | 2.0.1 | Замена upstream | Unmaintained; путь через promql-parser и parser tooling |

Доказательство SQLite: `Cargo.lock:3777`, дерево
`octofhir-canonical-core 0.2.13 → rusqlite 0.40.2 → libsqlite3-sys 0.38.2`;
есть также путь через deadpool-sqlite. Проверка features подтвердила `bundled`.
По [RustSec](https://rustsec.org/advisories/RUSTSEC-2026-0459.html), эксплуатация требует
открытия контролируемых менее доверенной стороной DB/WAL-файлов. Пути загрузки такой
пары через обычный HTTP API в этой проверке не установлено. Обновлять через совместимую
версию canonical-core/rusqlite; upstream указывает rusqlite 0.41 для исправленного SQLite.

`lru 0.16.4` приходит через `async-graphql 7.2.1`. Проверенная реализация persisted
query cache использует ключ String и get/put; сценарий panic в Drop ключа не показан.
В release-профиле также установлен `panic = "abort"` (`Cargo.toml`). Поэтому этот
[use-after-free advisory](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)
не следует объявлять подтверждённым удалённым RCE. Обновить async-graphql до версии,
использующей исправленный lru, либо согласовать upstream исправление зависимости.

`tests/library-consumer/Cargo.lock`: 0 vulnerabilities, 1 unsound warning. Это отдельный
lockfile, не основание считать основной сервер чистым.

Полные результаты: [cargo-audit.json](reports/security-2026-11-12/cargo-audit.json),
[consumer-cargo-audit.json](reports/security-2026-11-12/consumer-cargo-audit.json).

### SEC-08. npm: два Critical в отдельных компонентах, прочие alerts требуют triage

Счётчики npm: **2 Critical, 42 High, 34 Moderate, 9 Low**. В JSON 83 advisory records
и 77 уникальных GHSA URL. Эти числа считаются разными способами и не означают
87 удалённо эксплуатируемых проблем FHIR-сервера.

| Компонент | Находка | Установлено → исправлено | Применимость |
|---|---|---|---|
| docs / Astro | GHSA-26w7-cxv4-gfx2, RCE через AVIF | 5.18.2 → >=7.2.8 | Обработка недоверенного AVIF при оптимизации изображения |
| ui-kit / Storybook | CVE-2026-114125 / GHSA-f2ph-cmg5-wxc8 | 10.4.6 → >=10.6.2 | Работающий dev server, открытый браузером, и вредоносный сайт |
| UI / markdown-it | Несколько DoS, включая GHSA-gfr2-rv44-r23c | 14.2.0 → >=14.3.3 | Рендеринг недоверенного markdown соответствующей конфигурацией |
| UI / linkify-it | GHSA-v245-v573-v5vm, DoS | см. TSV → >=5.0.2 | Обработка недоверенного текста |
| UI / js-yaml | Несколько DoS | см. TSV → >=4.3.2 | Разбор недоверенного YAML |
| UI / Zod | GHSA-mrhx-c7xc-m3w5, memory exhaustion | 4.4.3 → >=4.6.6 | Валидация определённых чрезмерно больших коллекций |
| UI / React Router | GHSA-qwww-vcr4-c8h2 | 7.18.0 → >=7.18.2 | Advisory относится к RSC mode; его использование здесь не установлено |
| Dev / Vite, Vitest | File access / code injection advisories | см. TSV | Работающие dev/test-серверы; не статический Rust UI |

Условия Critical проверены по первоисточникам:
[Astro](https://github.com/withastro/astro/security/advisories/GHSA-26w7-cxv4-gfx2),
[Storybook](https://github.com/storybookjs/storybook/security/advisories/GHSA-f2ph-cmg5-wxc8).
Astro рекомендует 7.2.8 с Sharp 0.35.4. Статическая публикация docs на GitHub Pages
не доказывает наличие удалённого image optimizer; остаётся риск build pipeline при
обработке недоверенных изображений. Построенный статический Storybook не затронут
именно этой dev-server уязвимостью.

Исправление: обновлять отдельными совместимыми группами UI, dev tooling и docs;
для Astro учесть миграцию major и совместимость Starlight. Не применять слепые overrides
к нескольким major. Для каждой группы выполнить её build/typecheck и относящиеся tests,
затем повторить audit. Сначала закрыть применимые runtime-риски и работающий Storybook dev.

Полный список с версиями и dependency paths:
[npm-advisories.tsv](reports/security-2026-11-12/npm-advisories.tsv),
[pnpm-audit.json](reports/security-2026-11-12/pnpm-audit.json).

### SEC-09. Локальный Docker-образ содержит множество package-level alerts

Проверен существующий `octofhir-server-local:dev`:

- image ID: `sha256:16b4edce1a7d7a49727438455834e2840385eb0d5b61673fb1059ec542ab707f`;
- создан: 2026-10-30 20:07:25 +03:00; ОС Debian 12.15;
- 565 записей package/advisory: 26 Critical, 217 High, 206 Medium, 99 Low, 17 Unknown;
- 430 различных advisory IDs, в том числе один DLA ID;
- статусы: 3 fixed, 179 affected, 17 will_not_fix, 75 fix_deferred, 291 end_of_life.

Это старый локальный образ без установленной связи с проверенной Git-ревизией.
Число Critical отражает severity базы, а не доказанную достижимость через HTTP.
Trivy не обнаружил language-specific files, поэтому этот image scan **не заменяет**
проверку встроенного Rust-бинарника через Cargo.lock/SBOM.

Конкретное исправляемое совпадение: `apt` и `libapt-pkg6.0` 2.6.1,
**CVE-2026-109518**, исправлено в 2.6.2. По
[Debian Security Tracker](https://security-tracker.debian.org/tracker/CVE-2026-109518),
RCE требует атакующего на пути скачивания пакетов и работы APT с retries/redirects;
это риск установки пакетов/build, не доказанная HTTP-уязвимость работающего FHIR.
Третья запись с готовым обновлением — tzdata DLA-4867-1.

Пример необходимости triage: scanner также сообщает CVE-2023-45853 для zlib1g,
но [Debian](https://security-tracker.debian.org/tracker/CVE-2023-45853) поясняет,
что соответствующий MiniZip-код не собран в затронутый бинарный пакет bookworm.
Такие записи нельзя механически превращать в подтверждённые находки.

Исправление: собрать свежий runtime из поддерживаемой базы, убрать ненужные runtime
пакеты, просканировать новый image ID/digest и разбирать оставшиеся alerts по статусу
Debian и фактически используемой функции. Добавить OCI revision label, чтобы связать
сканирование образа с исходниками. Текущий Dockerfile использует non-root пользователя,
что полезно, но не исправляет уязвимые библиотеки само по себе.

Результаты: [image-trivy.json.gz](reports/security-2026-11-12/image-trivy.json.gz),
[image-advisories.tsv](reports/security-2026-11-12/image-advisories.tsv).
Из метаданных отчёта образа исключены environment и build history, не нужные для triage.

## Дополнительные замечания и существующие защиты

- External reference resolver делает предварительный DNS lookup
  (`reference_resolver.rs:375`), затем отправляет запрос обычным reqwest-клиентом
  (`reference_resolver.rs:150`). Проверенный адрес не закреплён за соединением:
  потенциальен DNS rebinding/TOCTOU. `validation.fetch_external_references` по умолчанию
  false (`config.rs:883`), динамический DNS-тест не выполнен. Перевести этот путь на
  общий egress API; redirects уже отключены.
- `notebook_api.rs:54` и `notebook_run.rs:496` читают Notebook напрямую из storage.
  Нужно отдельно проверить, что разрешение export/run соответствует object-level
  правилам обычного чтения Notebook. Само наличие глобальной API-политики не доказывает
  право на любой Notebook; полный сценарий IDOR в этой проверке не воспроизводился.
- QuickJS policy runtime ограничивает память, стек и время (`quickjs.rs:100`, строка 154).
  В проверенном модуле не обнаружено привязок shell/filesystem/network. Сам факт наличия
  `eval` не доказывает RCE. Пул сохраняет контексты между исполнениями — нужна отдельная
  проверка изоляции глобального состояния, если скрипты доверены разным субъектам.
- Automation/otter-код сейчас исключён из сборки: `lib.rs:16` содержит закомментированный
  `pub mod automations`. Он не объявлялся достижимым RCE-path текущего сервера.
- Извлечение package archives (`package_source/archive.rs`) уже ограничивает размер,
  число файлов и глубину пути; отвергает links/devices и escaping paths, создаёт файлы
  через `create_new`. Подтверждённого archive traversal на этом пути не найдено.
- Auth middleware по умолчанию требует учётные данные, anonymous_access по умолчанию false;
  ошибки policy обрабатываются с отказом. Наличие этих мер учитывалось в условиях находок.

## Порядок исправлений и проверка результата

1. **SQL и учётные записи:** закрыть консоль для недоверенных ролей, убрать известные
   bootstrap-пароли, разделить PostgreSQL-роли; исправить режимы исполнения SQL.
2. **Notebook:** прекратить пересылку Authorization внешним URL, ограничить headless REST,
   убрать raw HTML и `new Function` из origin UI.
3. **Исходящая сеть:** общий проверяющий API, все отправители через него; строгая политика
   и небольшой allowlist для production.
4. **Зависимости:** SQLite/canonical-core, затем UI parsers, Storybook/dev tooling и Astro.
5. **Образ и CI:** свежий поддерживаемый runtime и привязка digest к commit. В
   `.github/workflows/ci.yml` сейчас нет dependency/image audit; `justfile:102` даёт только
   ручной `cargo audit`. Добавить проверки PR и периодический audit, обрабатывать также
   warnings RustSec, а не только его vulnerability count.

Для исправлений нужны HTTP tests с непривилегированным пользователем и реальными
AccessPolicy: forbidden SQL, protected Notebook read/export/run, внешняя REST-ячейка
без Authorization, IP-literal/redirect/DNS SSRF, браузерный XSS-тест, лимиты SQL.
Сохранённые probes описывают уязвимое поведение текущей ревизии; после исправлений
они должны измениться/начать отклонять прежние примеры, это не нормальные regression tests.

Воспроизведение из корня репозитория:

```sh
python3 reports/security-2026-11-12/sql_probe.py
python3 reports/security-2026-11-12/sql_probe.py --postgres
python3 reports/security-2026-11-12/network_probe.py
node reports/security-2026-11-12/ui_probe.cjs
cargo audit --db /tmp/octofhir-advisory-db --json
pnpm audit --json
trivy --cache-dir /tmp/octofhir-trivy image --image-src docker --scanners vuln octofhir-server-local:dev
```

Для PostgreSQL probe нужны `initdb`, `pg_ctl`, `psql`; network probe использует уже
собранные debug rlibs. Проверки запускают только собственные временные службы и dummy
данные. Общий `cargo test --all --all-features` не запускался: production-код не менялся,
проверялись конкретные security-сценарии.
