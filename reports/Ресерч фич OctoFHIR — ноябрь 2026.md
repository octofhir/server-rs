# Ресерч фич для OctoFHIR и инструментов octofhir

Дата: 2026-11-01. Окно спроса: май 2025 — ноябрь 2026.

## Решение по приоритетам (2026-11-01)

- **SQL on FHIR — стратегический приоритет на ближайшие годы.** План: v3 и R6, терминология, DuckDB-раннер, `$sql-run`/`$sql-export`, Parquet. Подробности в п. 2.5.
- **fhir-diff — следующий кандидат в работу** (п. 2.4).
- **XML исключён** навсегда.

## Как собирались данные

- **Наши репо.** Прочитаны README и последние коммиты всех репозиториев организации, плюс `docs/competitive/*` (он частично устарел: `$import` уже сделан). В коде server-rs проверено, что уже есть.
- **Zulip (chat.fhir.org).** Использован анонимный spectator-эндпоинт `/json/messages` с narrow `channels:web-public`. Охват: 23 канала, около 35 тыс. сообщений и около 2 900 тем с 2025-05-01, плюс около 50 полнотекстовых поисков.
  - Счётчики тем получены regex-классификацией, поэтому их надо читать как «≥N».
  - Статический архив chat-archive.fhir.org остановился в апреле 2022. Ссылки на него ниже помечены «(архив, ≤2022)» и показывают давность проблемы, а не текущий спрос.
- **GitHub.** Через `gh` просмотрены открытые issues HAPI, org.hl7.fhir.core, IG Publisher, SUSHI/GoFSH, Firely, Microsoft FHIR Server, Medplum, google/fhir, fhir-data-pipes, Inferno, Synthea, sql-on-fhir-v2, cqframework, Pathling и matchbox.
  - Реакций в FHIR-репозиториях мало: максимум 6 👍. Поэтому основной сигнал — число комментариев, возраст issue и дубли в разных репо.
- **Стандарты и регуляторика.** Версии сверены по `package-list.json` HL7. Также использованы CMS, HealthIT.gov, Federal Register и Confluence-страницы Connectathon.
- **Ссылки.** Каждое утверждение о спросе имеет ссылку в разделе 5 (коды [Z], [G], [S], [T]). Всё без ссылки помечено **гипотезой**.

### Что уже есть у нас: чтобы не предлагать повторно

- **server-rs:**
  - системный поиск с `_type`;
  - горячая перезагрузка SearchParameter;
  - SMART v2 granular scopes (`patient/Observation.rs?category=…`);
  - Subscriptions с `$events`/`$status`/heartbeat, topic criteria и filters;
  - `$export`/`$import`;
  - терминологические операции `$expand`/`$lookup`/`$validate-code`/`$subsumes`/`$translate`/`$closure`;
  - `$everything`, `$graph`, `$evaluate-measure`;
  - массовая валидация, советник по индексам, Performance.
- **Чего в server-rs нет:** UDAP, `$member-match`, `Patient/$summary`, параметров Bulk STU3/STU4 (`_until`, `organizeOutputBy`, `allowPartialManifests`), `$bulk-submit`, `$expunge`/`_cascade`, рабочего `$convert`. XML не поддерживается, и это **решение, а не пробел**: сервер только JSON.
- **sof — терминологии нет ни в одном пути исполнения** (проверено по коду 2026-11-01, без запуска):
  - in-memory evaluator (`sof/crates/octofhir-sof/src/eval.rs:248`) создаёт `EvaluationContext` с `terminology_provider: None`, а движок собран на `EmptyModelProvider`. Поэтому `memberOf()`, `subsumes()`, `translate()` и `%terminologies` падают с ошибкой «memberOf function requires a terminology provider» (`fhirpath-rs/.../terminology/member_of_function.rs:207`), хотя сами функции в fhirpath-rs реализованы;
  - SQL-генератор для Postgres и DuckDB (`sql_generator/lower.rs:~500–527`) поддерживает только белый список функций (`exists`, `empty`, `count`, `join`, `extension`, `getReferenceKey`, `getResourceKey`, `lowBoundary`, `highBoundary`, `toString`), а на всё остальное отвечает «unsupported function memberOf()»;
  - server-rs: `$run` (`operations/sof/run.rs:282`) идёт через in-memory `CompiledView` и provider не передаёт, хотя у сервера есть свои `$expand` и `$validate-code`. `operations/sof/sql.rs` вызывает `SqlGenerator` без терминологии;
  - ValueSet и ConceptMap как SQL-отношения из v3 тоже не реализованы;
  - в v2 conformance (144/144) терминологических функций нет, поэтому на счёт тестов это не влияет. Но именно эту тему сейчас проектируют [Z24], и она входит в v3 [S8].
- **fhirpath-rs:** 1176/1176 локальных тестов. При этом lib.rs показывает устаревшие «82.7%» для `octofhir-fhirpath-compiler` [T25].
- **WASM:** сборка есть только у ucum-rs. У fhirpath, fhirschema и sof её нет.
- **maki:** уже умеет `fmt` (formatter) и GoFSH (decompiler).
- **mcp-rs:** только FHIRPath-инструменты, последний коммит в 2025-08.
- **term-squid:** терминологический сервер на Rust + SolidJS + Postgres с `$expand`/`$lookup`/`$validate-code`/`$translate` и CLI-загрузкой LOINC/ICD-10. Последний коммит 2025-09-30, проект заморожен.
- **reef:** порт тулкита на Gleam/BEAM, ранняя стадия: FHIRPath около 78% conformance, package manager, R4-типы через бэкенд inkgen.
- **Otlyra:** экспериментальный браузер на Rust поверх otter. К FHIR не относится, в кандидаты не идёт.

### Конкуренты на Rust: новое за 2025–2026

- **HeliosSoftware hfs** (MIT): сервер, terminology, FHIRPath, SQL-on-FHIR, R4–R6. Валидатора и CQL нет [T24].
- **reason-healthcare `rh`:** Rust CQL-движок, валидатор и SQL-on-FHIR [Z13][Z27].
- **ThalamiQ tlq-*** (MIT, FHIRPath VM + validator) и **FerroBRIDGE** (BUSL) [T22][T23].
- **Вывод:** «первые на Rust» — больше не аргумент. Отличаться придётся полнотой, публичными conformance-цифрами и WASM/CLI-удобством.

---

## 1. Короткий список

Трудоёмкость: **S** — до недели, **M** — 2–4 недели, **L** — 1–2 месяца, **XL** — больше 2 месяцев. Спрос оценён по шкале 1–5.

| # | Название | Тип | Проблема пользователя | Кто пользователь | Наши крейты | Труд. | Спрос | Чем отличаемся |
|---|---|---|---|---|---|---|---|---|
| 1 | **Офлайн-терминология для CI + «почему код не в ValueSet»** (`octofhir-tx`) | standalone + server | tx.fhir.org падает и блокирует CI. Версии и редакции CodeSystem и ValueSet пиняться непредсказуемо. На ошибку «not in ValueSet» нет объяснения | Авторы IG, CI-пайплайны, команды валидации | canonical-manager, терминология server-rs, ucum, term-squid | L (MVP M) | **5** — около 900 сообщений про tx.fhir.org в ≥240 темах; ≥15 тем об отказах; ≥55 тем «not in ValueSet» [Z1–Z7][G6–G9] | FHIRsmith — Node, нужен Java 17; коммерческий запуск, по README, требует HL7 partner status [T18]. Ontoserver коммерческий [T19]. Snowstorm Lite только для SNOMED [T20]. У нас один бинарь на пакетах + explain-trace |
| 2 | **Валидатор как CLI и GitHub Action с публичным parity-отчётом** | standalone | JVM: долгий старт, мегабайты памяти, OOM. Inferno-hosted validator закрывают | CI, разработчики IG, вендоры перед сертификацией | fhirschema, fhirpath, canonical-manager, ucum | M | **4** [Z8–Z12][G4–G5][G10–G13][T1–T4] | GoFHIR неполный, Records TS — 508/536, rh-validator — конкурент. Наша сила — FHIR Schema + targetProfile + QR + офлайн |
| 3 | **validator-diff: почему две валидации расходятся** | standalone (режим п.2) + страница Validation | CLI, validator.fhir.org и IG Publisher дают разные результаты. Версии валидатора ломают тех, кто раньше проходил | Те же + QA | fhirschema | S–M | **3** — ≥24 темы про inconsistent results [Z10][Z11][T2] | Аналогов не нашли. Заодно это наш инструмент паритета |
| 4 | **FHIR Toolbox MCP** (офлайн, знает профили) | standalone + server `/mcp` | MCP-серверы для FHIR — это обёртки CRUD над одним сервером. Нет инструментов «найди профиль или расширение», «провалидируй по IG», «посчитай FHIRPath» | Разработчики агентов, авторы IG с LLM | mcp-rs, fhirpath, fhirschema, canonical-manager, maki, sof | M | **4**, растёт — ≥45 тем про MCP, канал #Agents on FHIR с 2026-02 [Z37–Z40][G34][T28–T30][S20] | WSO2/Momentum/Aidbox MCP — CRUD (+validate у Aidbox). У нас офлайн-инструменты для авторинга, а в сервере — через SMART scopes, QuickJS-политики и AuditEvent |
| 5 | **fhir-diff: семантический diff пакетов и профилей + breaking-gate** | standalone | `validator -compare` с багами. Нужен ответ на вопрос «что изменилось между US Core 6.1 и 9». IG Publisher #194–197 открыты с 2020 | Авторы IG, интеграторы при апгрейде IG | fhirschema, canonical-manager, UX `lint --breaking` из banshee | M | **3** — ≥19 тем [Z14–Z16][G26][T5–T6][S3] | HealthSamurai fhir-profile-diff — веб-UI с LLM-объяснением [T7], не копируем. У нас CLI/CI-gate и оценка влияния на реальные данные |
| 6 | **SQL on FHIR v3 + быстрый раннер с SQL-запросами** | standalone + server | Нужен раннер ViewDefinition по NDJSON и Parquet. В v3 новые canonicals, SQLQuery и `$sql-run`. Терминология в SoF пока не решена, а у нас её нет вовсе: `memberOf` падает в обоих путях | Аналитики, data-инженеры | sof (144/144 v2), canonical-manager | M | **3** — ≥51 тема [Z21–Z24][G22–G25][S8][S16] | Pathling работает на JVM/Spark. Helios sof — Rust-конкурент. Aidbox `$materialize` не копируем |
| 7 | **Генератор валидных примеров по профилю (+ негативные)** | standalone + кнопка в консоли | Тестовые данные по произвольному IG. Synthea генерирует только US Core, на MII не работает | Тестировщики IG, разработчики валидаторов | fhirschema, fhirpath, терминология (п.1) | L | **2–3** [Z17][Z18][T8–T10][G33] | Synthea — клиническая симуляция. uscore-data-script только для US Core. Мы генерируем валидное by construction и мутанты на каждое ограничение |
| 8 | **WASM-бандл `@octofhir/wasm`** (validator + FHIRPath + UCUM + SoF) | standalone (npm) | Валидация в браузере без сервера (приватность, песочницы). Браузерные валидаторы поверхностные | Фронтенд, песочницы, авторы IG | fhirschema, fhirpath, ucum (WASM уже есть), sof | M–L | **3** [T11–T12][Z20] | Kotlin FHIRPath WASM — только FHIRPath. fhirpath.js — JS. Полного валидатора в WASM нет |
| 9 | **Bulk Data STU3/STU4 + `$bulk-submit`** | server | CMS-0057-F Provider Access и Payer-to-Payer к 2027-01-01. В STU4 kick-off только через POST, новый manifest, `$bulk-submit` | Плательщики, аналитика, миграции | server-rs (bulk, async jobs) | M | **3** — давит регуляторика [S5][S9][G18–G21][Z30][Z31] | Ранний STU4 на одном бинаре. Конкуренты обычно отстают на 1–2 версии IG (**гипотеза**) |
| 10 | **`$expunge` + `_cascade=delete`** | server | Физическое удаление и чистка истории (GDPR и право на забвение). На эти операции массово жалуются в HAPI и MS | Операторы | db-postgres | S–M | **3** [G14–G17] | У нас без побочного reindex. Перед удалением показываем dry-run с числом затронутых ресурсов |
| 11 | **Keyset-пагинация + постраничный POST `_search` для длинных next-ссылок** | server | Сейчас `_offset`: медленно на глубоких страницах. Next URL длиннее 2048 символов ломается за прокси | Клиенты больших выборок | search, db-postgres | S–M | **3** [G1–G3] | В HAPI и MS открыто годами |
| 12 | **Conformance-доказательства: Inferno US Core и Subscriptions в CI + полный CapabilityStatement** | server | Aligned Networks (с 2026-07-04) требует US Core API, полный CapStmt и уведомления через Subscriptions | Вендоры в США | server-rs | M–L | **3** [S12][S13] | Уже в roadmap как P0 (`docs/competitive/roadmap.md`). Здесь только поднимаем приоритет |
| 13 | **capcheck: проверка CapabilityStatement против реального поведения** | standalone | `$conforms` — «matter of research». Нет инструмента, который проверял бы, правду ли говорит сервер | QA серверов, интеграторы | fhirpath, fhirschema, canonical-manager | M | **2** — один сильный тред + требование Aligned Networks [Z36][T13–T14][S12] | Inferno пишет IG-специфичные тесты на Ruby, Touchstone коммерческий, fhir-capability-analyzer только статический |
| 14 | **`Patient/$summary` (IPS 2.0.1, EU PS)** | server | IPS STU2 вышел. По EHDS акты EEHRxF выходят к 2027-03-26, patient summary нужен к 2029 | EU-вендоры, IPS-проекты | `$everything`/`$graph` server-rs, fhirschema | M | **3** [S4][S14] | Делается поверх уже существующего `$everything` |
| 15 | **SMART-scoped MCP в сервере + AI Transparency labels** | server | Агент должен ходить в данные с ограничениями scopes и аудитом, а AI-записи — помечаться | Разработчики агентов | auth, QuickJS policies, audit | M | **3–4** [Z37][S17][S20][G34] | У Aidbox MCP — CRUD. У нас каждый tool call проходит через scopes и политики, пишет AuditEvent и ставит Provenance-тег по AI Transparency IG |
| 16 | **UDAP / FAST Security STU2** (DCR, B2B, tiered OAuth) | server | TEFCA Facilitated FHIR SOP v2 с 2026-03-08 | Участники TEFCA в США | octofhir-auth | L | **3** (только США) [S10][S11] | В OSS-серверах UDAP почти нет (**гипотеза**, не проверял) |
| 17 | **R6 readiness** (additional resources, новые коды CapStmt, тип `resource` у search param) | server + fhirschema + canonical-manager | R6 в normative-баллоте. SoF v3 уже на R6. Публикации ждём в 2027 (**гипотеза**) | Ранние внедренцы | все | M | **2** [S1][S2] | R4–R6 уже наш аргумент, его надо поддерживать |
| 18 | **Debug-trace для FHIRPath/CQL** по предложенному кросс-движковому формату | fhirpath-rs, cql-rs, консоль, VS Code | Отладка выражений по шагам. Формат trace предложен в 2026-09, реализаций нет | Авторы IG и CQL | fhirpath, cql | S–M | **2** [Z28][Z32][G27–G28] | Первыми реализуем предложение infra-wg. fhirpath-lab уже умеет step-debug |
| 19 | **FML/StructureMap-движок + конвертер R4↔R5↔R6 с отчётом о потерях** | standalone + server `$convert` | xver-пакеты в snapshot. Конвертеров почти нет, JS-движки заброшены | Миграции, SDC `$extract` | fhirpath, fhirschema, canonical-manager | XL | **3** [Z19][Z20][G29–G32][T15–T17] | Rust/WASM FML-движков нет вообще |
| 20 | **cql-rs: полный парсер (valueset/include/function) + LSP без JVM** | standalone | CQL-расширение VS Code требует Java 11 | Авторы eCQM | cql-rs | L | **2–3** [Z25–Z27][G35–G38][T26–T27] | Ниша сужается: cql-to-elm выпустил JS-бету, есть rh-cql на Rust |
| 21 | **FSH: `maki fmt` для вывода GoFSH + быстрый GoFSH** | standalone | Вывод GoFSH «не годится для людей», GoFSH API тратит около 5 с на ресурс | Авторы IG | maki (fmt и decompiler уже есть) | S | **2** — тред на 41 сообщение без решения [Z33][Z34] | Уже реализовано, осталось показать и замерить |

---

## 2. Топ-5 подробно

### 2.1. Офлайн-терминология + explain (`octofhir-tx`), пункт 1

**Зачем.** Это самый большой кластер боли в Zulip.
- tx.fhir.org упоминается примерно в 900 сообщениях за окно.
- Падения блокируют CI. Цитата: «This blocks CI builds», и ответ Grahame: «ci builds are pretty much blocked anyway» [Z2][Z1].
- Код «не в ValueSet», когда он там есть, из-за транзитивного VSAC-пакета. Не решено [Z3].
- Путаница с национальными редакциями SNOMED. Стандартный ответ — «run your own terminology server» [Z4].
- В GitHub: валидатор ходит в tx, даже когда ValueSet и CodeSystem лежат в пакетах [G6]; просят офлайн-инициализацию [G7]; IG Publisher падает, когда tx недоступен [G9].
- Существующие решения тяжёлые: FHIRsmith (Node + Java 17), Ontoserver (коммерческий), Snowstorm (только SNOMED) [T18–T20].
- Готовый эталон для проверки есть: HL7 tx-ecosystem test suite [T21].

**MVP за 1–2 недели.**
1. Бинарь `octofhir-tx serve --packages hl7.fhir.r4.core,hl7.terminology.r4,us.core@6.1.0` на SQLite-хранилище canonical-manager. Логику операций переиспользуем из `operations/terminology` server-rs: её стоит вынести в отдельный крейт.
2. Минимум, при котором HL7 Java validator и IG Publisher работают с `-tx http://localhost:…`: `metadata` + `TerminologyCapabilities`, `$validate-code`, `$expand`, `$lookup`, батч-валидация кодов.
3. Explain-trace в ответе `$validate-code`, в отдельном параметре Parameters: какой ValueSet и какая версия выбраны и из какого пакета; какой include/exclude отбросил код; ближайшие похожие коды; откуда приехала версия (пин, транзитивная зависимость, latest).
4. Прогон tx-ecosystem suite и публикация процента в README.
5. Docker-образ и Action `octofhir/setup-tx`.

**Что дальше.**
- Lockfile версий терминологии (`tx.lock`), чтобы прогоны в CI были детерминированными.
- Загрузчики SNOMED RF2 и LOINC из файлов пользователя: данные не поставляем, только загрузчики.
- Кнопка «Explain» в Terminology workbench и в сообщениях Validation.
- WASM-вариант для валидации в браузере (п. 8).
- term-squid либо влить в этот проект, либо официально заархивировать.

**Риски.**
- Java-валидатор ожидает от tx-сервера много неочевидного. Ровно для этого и существует tx-ecosystem suite: процент из него и есть метрика.
- Лицензии SNOMED и LOINC.
- Пересечение с Aidbox Termbox. Отличие в форме продукта: офлайн-бинарь для CI с explain и lockfile, а не сервис.

**Как проверить спрос до разработки.**
- Сначала выложить Docker-образ, который отдаёт только `$validate-code`/`$expand` по пакетам. Предложить его в живых тредах [Z1][Z6][Z3] как обходной путь для CI.
- Через 2 недели смотреть на pulls, stars и ответы.
- Отдельно спросить 3–5 авторов IG из этих тредов, готовы ли они переключить `-tx`.
- Не повторять прошлые ошибки: наши анонсы [Z35] остались без ответа. Работает только ответ в существующий тред с проблемой.

### 2.2. Валидатор как CLI и Action + validator-diff, пункты 2 и 3

**Зачем.**
- Inferno закрывает Resource Validator Service «as soon as August 2026» [T3]. Обновление Inferno 1.0.78 ломает системы, которые раньше проходили [T2].
- Боль JVM: OOM на старте при увеличении числа пакетов, и ответ «нет способа оценить, просто пробуйте» [Z8]; поддержку Java 11 сняли [Z9].
- ≥24 темы о том, что результаты расходятся. Одна осталась без ответа [Z10], в другой ответ — скрытые дефолты, описанные только на Confluence [Z11].
- В GitHub просят валидатор без зависимостей и без JVM [G10–G13].
- Планку задают конкуренты, которые публикуют parity: Records TS — 508/536 [Z12], rh-validator на Rust [Z13], GoFHIR [T1].

**MVP за 1–2 недели.**
1. `octofhir validate <file|dir|ndjson> --ig us.core@6.1.0 [--tx offline|URL]`. Вывод: OperationOutcome, SARIF и GitHub annotations.
2. `--compare-with java` (если рядом лежит `validator_cli.jar`). Нормализует issues обоих движков по severity, location и message-id и печатает diff: «у нас есть, у HL7 нет» и наоборот. Это и есть validator-diff.
3. Таблица parity по fhir-test-cases по категориям (structure, slicing, terminology, invariants, references) в README. Раннеры `official-fhir-runner` и `validation-lab` уже есть.
4. GitHub Action.

**Что дальше.**
- Validator-diff между релизами HL7 validator: дашборд «что сломается при апгрейде».
- WASM.
- Объяснение сообщений через LLM при стабильных rule-based сообщениях. Это согласуется с позицией сообщества [Z40].

**Риски.**
- Публичный parity покажет дыры. Это плата за доверие, а не причина молчать.
- Message-id у движков разные, поэтому нужна таблица соответствий.

**Как проверить спрос.**
- Опубликовать parity-цифры и Action. Написать в #FHIR Validator и пользователям Inferno, которых затронуло закрытие.
- Сигналы: установки Action и issues вида «расходится с HL7 тут».

### 2.3. FHIR Toolbox MCP, пункты 4 и 15

**Зачем.**
- Канал #Agents on FHIR открыт 2026-02-26 и насчитывает 38 тем с еженедельными созвонами. ≥45 тем про MCP.
- На вопрос «что класть в `tools`?» стандартного ответа нет [Z37].
- XIG «не дружит с LLM», MCP поверх реестра профилей предлагали, но никто не сделал [Z39].
- Позиция сообщества по AI-валидации — детерминированный валидатор плюс AI для объяснений [Z40].
- В Medplum MCP-инструменты — заглушки [G34]. У WSO2 и Momentum по 100+ звёзд, но это CRUD [T28–T29].
- HL7 AI Office «ожидает» работу по MCP и A2A, на CAT43 был трек AI Transparency с MCP [S17][S20].

**MVP за 1–2 недели.** Переписать mcp-rs в `octofhir-mcp` (stdio и HTTP) без обязательного сервера, с инструментами:
- `validate_resource(ig)`;
- `fhirpath_eval`;
- `find_profile` и `find_extension` по индексу canonical-manager — закрывает XIG-gap;
- `expand_valueset` и `validate_code` через п. 1;
- `fsh_compile` и `fsh_lint` через maki;
- `sof_run`;
- `explain_issue`, который возвращает структурированный контекст правила.

**Что дальше.** Эндпоинт `/mcp` в server-rs:
- каждый tool call проходит через SMART scopes и QuickJS-политики;
- AuditEvent с идентичностью агента;
- записи от агента помечаются по AI Transparency IG через Provenance и security label [S17].

Это и есть отличие от Aidbox MCP: у них CRUD + validate [T30].

**Риски.**
- Хайп и быстрые изменения спецификации MCP.
- Качество инструментов придётся мерить eval-сценариями.

**Как проверить спрос.**
- Опубликовать в MCP-реестрах и показать на созвоне #Agents on FHIR.
- Через 2 недели смотреть на установки и звёзды по сравнению с WSO2 и Momentum.

### 2.4. fhir-diff: семантический diff и breaking-gate, пункт 5

**Зачем.**
- В `validator -compare` binding и datatypes печатаются как «??». Ответ Grahame: «my bug… on my list» [Z14].
- Вопрос «есть ли diff PH Core vs US Core?» получил в ответ только ссылку на `-compare` [Z15].
- Josh Mandel сделал fhir-diff «для LLM» [Z16].
- Правила сравнения версий в IG Publisher открыты с 2020 года [G26].
- Повод из регуляторики: CMS пинит US Core 6.1.0, а SVAP 2026 разрешает US Core 9 [S3][S12]. Внедренцам нужен ответ на вопрос «что сломается при апгрейде».

**MVP за 1–2 недели.**
1. `fhir-diff us.core@6.1.0 us.core@9.0.0 [--profile …]` с выводом в Markdown, JSON и SARIF.
2. Классификация изменений:
   - breaking: min вырос, max уменьшился, добавлен mustSupport, binding стал строже, новый invariant, удалён тип, slicing закрыт;
   - compatible: всё остальное.
3. `--fail-on breaking` для CI.

FHIR Schema — нормализованное дерево элементов, на нём diff проще, чем на snapshot. UX берём из `banshee lint --breaking`.

**Что дальше.**
- Оценка влияния на данные: «сколько из ваших N ресурсов станут невалидными» через п. 2.
- Diff R4 → R6 по ресурсам.
- Просмотр в консоли.

**Риски.**
- Семантика diff для slicing неоднозначна.
- Спрос средний (2–3).

**Как проверить спрос.** Прогнать US Core 6.1 → 9, опубликовать отчёт и ответить в [Z15] и в #IG creation. Мерить по реакциям редакторов US Core.

### 2.5. SQL on FHIR v3 + быстрый SQL-раннер, пункт 6

**Зачем.**
- ≥51 тема.
- Просят standalone-раннер ViewDefinition по NDJSON из bulk export, потому что JS reference implementation «not optimized» [Z22].
- Операции `$viewdefinition-run`/`$sqlquery-run` внутренне противоречивы, заведено 6 issues [Z23].
- Терминология в SoF ещё проектируется [Z24].
- Ballot v3 вышел 2026-08-14 и построен на R6: новые canonicals и package id, SQLQuery/SQLView, `$sql-run`/`$sql-export`, ValueSet и ConceptMap как SQL-отношения [S8].
- На CAT42 был трек SQL on FHIR [S16].

**Состояние терминологии сейчас.** Терминологии нет ни в in-memory, ни в SQL-пути (см. «Что уже есть»). Это самый дешёвый вход в тему, которую сообщество ещё только проектирует [Z24].

**Принцип сборки sof.** Любой опциональный бэкенд — это cargo-фича, как уже сделан `parquet` (`default = []`). Ядро без фич содержит парсинг ViewDefinition, in-memory evaluator, генерацию SQL для всех диалектов (это просто строки) и lint, и должно собираться под wasm32. Фичи:
- `postgres` — sqlx-executor. Сейчас `sqlx-postgres` — обязательная зависимость ядра, его нужно вынести за фичу. server-rs будет включать её явно;
- `duckdb` — встроенный executor;
- `parquet` — уже есть;
- `tx-remote` — HTTP TerminologyProvider;
- `tx-packages` — терминология из пакетов через canonical-manager.

Фичи CLI пробрасываются в фичи библиотеки.

**MVP за 1–2 недели.**
0. Терминология в sof:
   - **in-memory:** `CompiledView::compile_with(…, terminology: Option<Arc<dyn TerminologyProvider>>)` и передача provider в `EvaluationContext`. В server-rs `$run` получает локальный provider поверх своих `$expand`/`$validate-code`. В CLI — флаг `--tx <url|packages>`. Займёт дни;
   - **SQL:** lowering `x.memberOf('vs')` в `EXISTS (SELECT 1 FROM sof_vs_expansion e WHERE e.valueset = 'vs' AND e.system = … AND e.code = …)`. Таблицу `sof_vs_expansion` сервер наполняет из `$expand`, CLI — из пакетов canonical-manager. Это прямой шаг к v3, где ValueSet — SQL-отношение. Займёт 1–2 недели.
1. Поддержка v3-пакета и canonicals с обратной совместимостью с v2.
2. `octofhir-sof query views/ q.sql --input export/`: встроенный DuckDB (крейт `duckdb`, bundled, за feature-флагом) как ещё одна реализация уже существующего `SqlExecutor`. DuckDB-диалект генератора уже есть (`sql_generator/dialect.rs`). NDJSON и Parquet читаются через `read_json`/`read_parquet`, поверх них выполняются SQL view, а затем SQLQuery пользователя. Получается SQLQuery без сервера и без Postgres. Риск: bundled DuckDB долго собирается (C++), отсюда и feature-флаг.
3. В server-rs — `$sql-run` поверх Postgres.
4. Публикация результатов v3 conformance.

**Что дальше.**
- ValueSet и ConceptMap как таблицы из canonical-manager.
- Parquet с lossless-кодированием [G24].
- `$sql-export` через async jobs.

**Риски.**
- Спецификация будет меняться в ходе баллота.
- Helios sof — прямой конкурент на Rust.
- Materialized views (`$viewdefinition-materialize`, sof #326 по модели Aidbox) не делаем, чтобы не копировать.

**Как проверить спрос.** Первыми выложить v3-результаты в #analytics on FHIR, поучаствовать в треке ближайшего Connectathon и мерить по скачиваниям CLI.

---

## 3. Быстрые победы: на дни

1. **Поправить описание `octofhir-fhirpath-compiler` на lib.rs и crates.io.** Там до сих пор «82.7% compliance», а локально 1176/1176 [T25]. Если крейт больше не используется, пометить его deprecated.
2. **Опубликовать parity fhirschema по fhir-test-cases** одной таблицей в README. Раннеры уже есть.
3. **`maki fmt` для вывода GoFSH.** Ответить в треде «FSH Reformatter» (41 сообщение, решения нет) [Z33] и замерить скорость `maki gofsh` против GoFSH API, который тратит около 5 с на ресурс [Z34].
4. **Bulk STU3 без STU4.** Добавить `_until`, kick-off через POST с Parameters, `organizeOutputBy=Patient`, `allowPartialManifests` [S9]. Это малые правки поверх существующего `$export`.
5. **Explain в Terminology workbench.** Показывать, какая версия CodeSystem и ValueSet и из какого пакета участвовала в `$validate-code`. Это первый шаг к п. 1 без нового бинаря.
6. **Минимальный `$expunge`:** для одного экземпляра и с очисткой истории, только с dry-run и admin scope [G14–G15].
7. **Страница в документации «без `$reindex`».** Поиск в server-rs идёт SQL по JSONB, новый SearchParameter подхватывается hook-ом без перестройки. В HAPI и MS reindex — постоянная боль [G16][G17]. Это преимущество, его нужно сформулировать: лучше одним абзацем про функциональные индексы и советник по индексам.
8. **Прогнать Inferno Subscriptions test kit** (если он есть для R5/Backport — **проверить**) и опубликовать результат. `$events`, `$status` и heartbeat уже реализованы, а Aligned Networks требует уведомлений через Subscriptions [S13].
9. **`memberOf()` в `$run` и CLI sof.** Сейчас вызов падает: provider не передаётся, хотя функция есть в fhirpath-rs, а терминология есть в сервере. Достаточно пробросить `TerminologyProvider` в `EvaluationContext` (`sof/.../eval.rs:248`) и подключить его в `operations/sof/run.rs`. Займёт дни, а заметно сразу: это первый работающий терминологический ViewDefinition [Z24].
10. **FHIRPath 3.0 STU** (опубликован 2026-07-28 [S15]): сверить новые функции с fhirpath-rs и закрыть разницу. Объём неизвестен — **гипотеза**: дни.

---

## 4. Что не делать и почему

- **Дальше вкладываться в FHIRPath LSP и VS Code extension.** В Zulip ноль запросов на FHIRPath LSP. #fhirpath — это семантика спецификации, а не тулинг [Z32]. Есть fhirpath-lab и fhirpath.js. Поддерживать, но не развивать.
- **FSH LSP.** Запросов нет. Есть @aehrc/fsh-language-server v0.8.6 [T27]. В maki ценнее formatter и скорость.
- **Прокси или инспектор FHIR-трафика.** В Zulip спроса не нашли, около 10 слабых упоминаний. Сигнал только в блогах про OpenTelemetry [T31].
- **Da Vinci payer pack** (`$member-match`, PAS, CRD/DTR). Регуляторный нажим сильный: дедлайн 2027-01-01 [S5]. Но покупатели — плательщики, а они выбирают энтерпрайз-вендоров с сертификацией. Поверхность IG огромная (CRD, DTR, PAS, PDex, Plan-Net, Formulary), а у одиночного OSS-инстанса нет каналов продаж. Если делать, то позже и только `$member-match` как вход.
- **`$reindex`.** Архитектурно не нужен (см. быстрые победы, п. 7).
- **Multi-tenancy и горизонтальное масштабирование.** Вне рамок задачи.
- **XML.** Не поддерживаем совсем, это решение команды: формат устаревший. Спроса в источниках за окно не нашли, а g10 и Aligned Networks используют JSON. Пункт W7/G9 из `docs/competitive` (XML) нужно убрать из roadmap.
- **То, что уже похоже на Aidbox.** Ноутбуки, DB-консоль со статистикой search params и batch-validate у нас уже есть, а у Aidbox — notebooks с SQL и charts, Database page, `$batch-validate` [T30]. Не развивать их в сторону паритета с Aidbox: не делать shareable HTML-экспорт ноутбуков, чарты в SQL-ячейках, `$materialize`. Развиваться туда, где Aidbox нет: офлайн, CLI, WASM, explain, публичные conformance-цифры.
- **Позиционировать cql-rs как отдельный продукт.** Появились rh-cql на Rust [Z27] и cql-to-elm 4.0 для JS «без Java» [Z26]. Парсер доделать (valueset, include, function) ради нашей CQL-консоли и `$evaluate-measure`, но гонку за standalone-CQL сейчас не начинать.
- **FML и конвертер R4↔R6 до выхода R6.** Объём XL. xver-пакеты ещё в snapshot [T15], StructureMap в R6 вынесен в incubator [S2]. Вернуться к этому, когда R6 будет опубликован.
- **Клиническая симуляция в духе Synthea.** Synthea её уже покрывает. Если делать генератор, то по профилям (п. 7), а не по сценариям болезней.

### Отдельная пометка: SDC-формы (`$populate`/`$extract`/рендеринг)

**За:**
- SDC 4.0.0 вышел 2026-03 [S4]. На SDC стоит DTR, а это prior auth по CMS-0057-F.
- `$extract` через FML в браузере просят сразу, как только появляется JS-конвертер [Z20].
- SDC сильно опирается на FHIRPath, а это наш сильный крейт.

**Против:**
- Формы — ключевой продукт Aidbox (Aidbox Forms и Formbox с AI-конструктором) [T30]. Риск сходства максимальный.
- Хорошие open-source рендереры уже есть: LHC-Forms (NLM), CSIRO Smart Forms. Ссылки известные, но в этом ресерче не перепроверены.
- Рендеринг — большой UI-проект вне нашей специализации.

**Если когда-нибудь возвращаться:** только headless-движок `$populate`/`$extract` на Rust/WASM, как библиотеку для чужих рендереров, без конструктора и рендерера. Решение «отложить» остаётся в силе.

---

## 5. Источники

Счётчики тем Zulip получены regex-классификацией по 23 web-public каналам за 2025-05-01…2026-11-01, их нужно читать как «≥N». Даты указаны по первому сообщению из выборки.

### Zulip (chat.fhir.org)

- **[Z1]** #tooling «tx.fhir.org down?», 2026-06-16, 59 сообщений — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/tx.2Efhir.2Eorg.20down.3F/near/603768279
- **[Z2]** #IG creation «tx.fhir.org down», 2026-03-16 — https://chat.fhir.org/#narrow/channel/179252-IG.20creation/topic/tx.2Efhir.2Eorg.20down/near/579682165
- **[Z3]** #IG creation «Code returning not in ValueSet validation error, when it is», 2026-08-24 — https://chat.fhir.org/#narrow/channel/179252-IG.20creation/topic/Code.20returning.20not.20in.20ValueSet.20validation.20error.2C.20when.20it.20is/near/618623041
- **[Z4]** #FHIR Validator «validation errors with national SCT editions», 2026-03-20 — https://chat.fhir.org/#narrow/channel/291844-FHIR.20Validator/topic/validation.20errors.20with.20national.20SCT.20editions/near/580633026
- **[Z5]** #FHIR Validator «Validator and tx.fhir.org code system version questions», 2026-07, 31 сообщение — https://chat.fhir.org/#narrow/channel/291844-FHIR.20Validator/topic/Validator.20and.20tx.2Efhir.2Eorg.20code.20system.20version.20questions/near/611931719
- **[Z6]** #IG creation «-tx n/a», 2026-04-11, 57 сообщений — https://chat.fhir.org/#narrow/channel/179252-IG.20creation/topic/-tx.20n.2Fa/near/584930901
- **[Z7]** #tooling «New tx.fhir.org» (FHIRsmith открыт), 2026-01-29 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/New.20tx.2Efhir.2Eorg/near/570895792
- **[Z8]** #implementers «Large JVM heap size for java FHIR validator», 2025-11-06 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Large.20JVM.20heap.20size.20for.20java.20FHIR.20validator/near/554177216
- **[Z9]** #tooling «Validator minimal java version», 2025-09-10 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/Validator.20minimal.20java.20version/near/538617674
- **[Z10]** #FHIR Validator «Inconsistent Results Using Same Validator Versions», 2025-12-05 — https://chat.fhir.org/#narrow/channel/291844-FHIR.20Validator/topic/Inconsistent.20Results.20Using.20Same.20Validator.20Versions/near/562080625
- **[Z11]** #FHIR Validator «Differences in validation results», 2025-07 — https://chat.fhir.org/#narrow/channel/291844-FHIR.20Validator/topic/Differences.20in.20validation.20results/near/526628074
- **[Z12]** #conformance «Records FHIR Validator 0.3.0» (TS, 508/536), 2026-06-23 — https://chat.fhir.org/#narrow/channel/179177-conformance/topic/Records.20FHIR.20Validator.200.2E3.2E0/near/606104560
- **[Z13]** #tooling «New set of FHIR tooling» (reason-healthcare rh, Rust), 2026-07-03 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/New.20set.20of.20FHIR.20tooling/near/608091274
- **[Z14]** #tooling «validator -compare features: bindings, datatypes, excel», 2025-05-14 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/validator.20-compare.20features.3A.20bindings.2C.20datatypes.2C.20excel/near/518164155
- **[Z15]** #implementers «Compare resources», 2025-08-24 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Compare.20resources/near/535874872
- **[Z16]** #conformance «R4->R6 migration analysis by resource», 2025-05-09 — https://chat.fhir.org/#narrow/channel/179177-conformance/topic/R4-.3ER6.20migration.20analysis.20by.20resource/near/517005895
- **[Z17]** #implementers «Test Data Generation (Test Data Factories) based on IG», 2025-11-25 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Test.20Data.20Generation.20.28Test.20Data.20Factories.29.20based.20on.20IG/near/560180946
- **[Z18]** #implementers «Getting Started with FHIR IG and Test Data Factories», 2025-06-02 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Getting.20Started.20with.20FHIR.20IG.20and.20Test.20Data.20Factories/near/521773337
- **[Z19]** #implementers «Which package to import for cross version extensions», 2025-09-16, 46 сообщений — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Which.20package.20to.20import.20for.20cross.20version.20extensions/near/539774491
- **[Z20]** #implementers «FML FHIR resource version converter (JavaScript)», 2026-09-29 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/FML.20FHIR.20resource.20version.20converter.20.28JavaScript.29/near/627890145
- **[Z21]** #analytics on FHIR «$run operation questions», 2025-06-17 — https://chat.fhir.org/#narrow/channel/179219-analytics.20on.20FHIR/topic/.24run.20operation.20questions/near/524473162
- **[Z22]** #analytics on FHIR «SQL on FHIR (only Parser)», 2025-05-02 — https://chat.fhir.org/#narrow/channel/179219-analytics.20on.20FHIR/topic/SQL.20on.20FHIR.20.28only.20Parser.29/near/515786419
- **[Z23]** #analytics on FHIR «Spec inconsistencies — 6 issues filed», 2026-06-02 — https://chat.fhir.org/#narrow/channel/179219-analytics.20on.20FHIR/topic/Spec.20inconsistencies.20.E2.80.94.206.20issues.20filed/near/599553752
- **[Z24]** #analytics on FHIR «Terminology in SQL on FHIR», 2026-09-08 — https://chat.fhir.org/#narrow/channel/179219-analytics.20on.20FHIR/topic/Terminology.20in.20SQL.20on.20FHIR/near/622446640
- **[Z25]** #cql «Feedback developing a new CQL-to-ELM translator», 2025-12-06 — https://chat.fhir.org/#narrow/channel/179220-cql/topic/Feedback.20developing.20a.20new.20CQL-to-ELM.20translator/near/562242827
- **[Z26]** #cql «CQL Release 4.0.0 (CQL to ELM for Javascript)», 2025-11 — https://chat.fhir.org/#narrow/channel/179220-cql/topic/CQL.20Release.204.2E0.2E0.20.28CQL.20to.20ELM.20for.20Javascript.29/near/554805956
- **[Z27]** #cql «New CQL Engine» (rh-cql, Rust), 2026-04-23 — https://chat.fhir.org/#narrow/channel/179220-cql/topic/New.20CQL.20Engine/near/590515687
- **[Z28]** #fhir/infrastructure-wg «Debug tracing», 2026-09 — https://chat.fhir.org/#narrow/channel/179280-fhir.2Finfrastructure-wg/topic/Debug.20tracing/near/625466262
- **[Z29]** #subscriptions «Backport $events return body», 2026-10-09 — https://chat.fhir.org/#narrow/channel/179229-subscriptions/topic/Backport.20.24events.20return.20body/near/630295500
- **[Z30]** #bulk data «Bulk Upload? Simpler alternative to Bulk Submit», 2026-09-08 — https://chat.fhir.org/#narrow/channel/179250-bulk.20data/topic/Bulk.20Upload.3F.20.20Simpler.20alternative.20to.20Bulk.20Submit/near/622602238
- **[Z31]** #bulk data «_since is polling - should we re-focus on notifications?», 2025-05, 59 сообщений — https://chat.fhir.org/#narrow/channel/179250-bulk.20data/topic/_since.20is.20polling.20-.20should.20we.20re-focus.20on.20notifications.3F/near/519196811
- **[Z32]** #fhirpath «fhirpath-lab» (step-debugger), 2025-06 — https://chat.fhir.org/#narrow/channel/179266-fhirpath/topic/fhirpath-lab/near/523078207
- **[Z33]** #shorthand «FSH Reformatter», 2025-11-24, 41 сообщение — https://chat.fhir.org/#narrow/channel/215610-shorthand/topic/FSH.20Reformatter/near/559083425
- **[Z34]** #shorthand «goFsh API is slow...», 2025-11 — https://chat.fhir.org/#narrow/channel/215610-shorthand/topic/goFsh.20API.20is.20slow.2E.2E.2E/near/554518701
- **[Z35]** #tooling «Experimental FHIR tooling» (наш пост без ответов), 2026-07-06 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/Experimental.20FHIR.20tooling/near/608497997
- **[Z36]** #conformance «Testing the claims of a CapabilityStatement», 2026-09-22 — https://chat.fhir.org/#narrow/channel/179177-conformance/topic/Testing.20the.20claims.20of.20a.20CapabilityStatement/near/626147687
- **[Z37]** #social «Expose your FHIR server as a MCP server», 2025-07-22, 37 сообщений — https://chat.fhir.org/#narrow/channel/179160-social/topic/Expose.20your.20FHIR.20server.20as.20a.20MCP.20server/near/530203962
- **[Z38]** #AI/ML «AI Ready IGs», 2025-09-22, 51 сообщение — https://chat.fhir.org/#narrow/channel/323443-Artificial.20Intelligence.2FMachine.20Learning.20.28AI.2FML.29/topic/AI.20Ready.20IGs/near/540702489
- **[Z39]** #AI/ML «AI for IG writers to use to find prior art/profiles», 2025-09-24 — https://chat.fhir.org/#narrow/channel/323443-Artificial.20Intelligence.2FMachine.20Learning.20.28AI.2FML.29/topic/AI.20for.20IG.20writers.20to.20use.20to.20find.20prior.20art.2Fprofiles/near/541150570
- **[Z40]** #implementers «Looking for feedback on an AI-powered FHIR validation tool», 2026-06-30 — https://chat.fhir.org/#narrow/channel/179166-implementers/topic/Looking.20for.20feedback.20on.20an.20AI-powered.20FHIR.20validation.20tool/near/607444407
- **[Z41]** #tooling «IG Publisher Performance» (US Core cold build 22→11 мин), 2026-02-21 — https://chat.fhir.org/#narrow/channel/179239-tooling/topic/IG.20Publisher.20Performance/near/575049121

### GitHub

Формат: 👍 / все реакции / комментарии; год создания.

**Поиск и пагинация**
- **[G1]** hapi-fhir #685 «Search across multiple resources», 6/6/6, 2017 — https://github.com/hapifhir/hapi-fhir/issues/685
- **[G2]** hapi-fhir #1940 «No stateless pagination», 4/4/1, 2020 — https://github.com/hapifhir/hapi-fhir/issues/1940
- **[G3]** microsoft/fhir-server #1567 «Next URL > 2048 chars», 1/1/18 — https://github.com/microsoft/fhir-server/issues/1567. Там же #1514 (нет first/last/previous, 0/0/11) и hapi #6109 (таймауты параллельных страниц, 1/1/16).

**Мульти-версия**
- **[G4]** hapi-fhir #6013 «Latest Docker image doesn't work with R5», 5/5/2 — https://github.com/hapifhir/hapi-fhir/issues/6013
- **[G5]** microsoft/fhir-server #3368 «Can we use R5», 0/0/11 — https://github.com/microsoft/fhir-server/issues/3368

**Валидатор и терминология**
- **[G6]** org.hl7.fhir.core #2272 «Validator depends on tx even when VS/CS are packaged locally», 2025-12 — https://github.com/hapifhir/org.hl7.fhir.core/issues/2272
- **[G7]** org.hl7.fhir.core #2572 «Initialize ValidationEngine completely offline», 2026-08 — https://github.com/hapifhir/org.hl7.fhir.core/issues/2572
- **[G8]** org.hl7.fhir.core #2262 «Perf degradation, multiple codings vs tx», 0/0/11 — https://github.com/hapifhir/org.hl7.fhir.core/issues/2262. Там же #2691: remote tx грузит весь CodeSystem, 2026-10.
- **[G9]** IG Publisher #1378 (сборка падает при недоступном tx) и #1359 (CACHE_ID_UNKNOWN), 2026 — https://github.com/HL7/fhir-ig-publisher/issues/1378
- **[G10]** org.hl7.fhir.core #2506 «core should not depend on hapi-fhir-base» — https://github.com/hapifhir/org.hl7.fhir.core/issues/2506
- **[G11]** google/fhir #22 «Go profiling + validation», 2/2/11, 2020 — https://github.com/google/fhir/issues/22
- **[G12]** hapi-fhir #7369 «GraalVM native compilation», 4/4/5, 2025-11 — https://github.com/hapifhir/hapi-fhir/issues/7369
- **[G13]** org.hl7.fhir.core #517 «forward-compatible flag», 0/0/14, 2021 — https://github.com/hapifhir/org.hl7.fhir.core/issues/517

**Удаление и reindex**
- **[G14]** hapi-fhir #2500 «_cascade=delete isn't working», 3/3/10 — https://github.com/hapifhir/hapi-fhir/issues/2500
- **[G15]** hapi-fhir #4158 ($expunge внутри transaction) и #4092 (ConstraintViolation при $expunge) — https://github.com/hapifhir/hapi-fhir/issues/4158
- **[G16]** microsoft/fhir-server #3684 «Reindex job stuck at Queued», 0/0/14 — https://github.com/microsoft/fhir-server/issues/3684. Там же #3487: новый SearchParameter ломает остальные до рестарта.
- **[G17]** hapi-fhir #7017 «$reindex walks every type», 4/4/0, 2025 — https://github.com/hapifhir/hapi-fhir/issues/7017

**Bulk Data**
- **[G18]** medplum #10479 «$export never fills `deleted`», 2026-09 — https://github.com/medplum/medplum/issues/10479
- **[G19]** hapi-fhir-jpaserver-starter #871 «Binary в S3-совместимом хранилище», 2/2/4 — https://github.com/hapifhir/hapi-fhir-jpaserver-starter/issues/871
- **[G20]** google/fhir-data-pipes #1431 «Failure to fetch all resources from HAPI», 0/0/20 — https://github.com/google/fhir-data-pipes/issues/1431
- **[G21]** aehrc/pathling #2467 «basic Bulk Data export server», 0/0/4 — https://github.com/aehrc/pathling/issues/2467

**SQL on FHIR**
- **[G22]** sql-on-fhir-v2 #326 «$viewdefinition-materialize», 0/0/8, 2026-03 — https://github.com/FHIR/sql-on-fhir-v2/issues/326
- **[G23]** sql-on-fhir-v2 #276 и #277: async `$run` и прогресс — https://github.com/FHIR/sql-on-fhir-v2/issues/276
- **[G24]** aehrc/pathling #2367 «Parquet on FHIR» — https://github.com/aehrc/pathling/issues/2367
- **[G25]** aehrc/pathling #437: extensions на примитивах, 0/0/16 — https://github.com/aehrc/pathling/issues/437

**IG authoring и отладка выражений**
- **[G26]** IG Publisher #194, #196, #197: правила сравнения версий, открыты с 2020 — https://github.com/HL7/fhir-ig-publisher/issues/194
- **[G27]** HL7/fhirpath.js #4 «error reporting with positions», 2019 — https://github.com/HL7/fhirpath.js/issues/4
- **[G28]** firely-net-sdk #3313 (статический анализ FHIRPath) и #3193 (отладка) — https://github.com/FirelyTeam/firely-net-sdk/issues/3313

**Кросс-версия**
- **[G29]** ahdis/matchbox #265 «FML transforms between FHIR versions», 0/0/4 — https://github.com/ahdis/matchbox/issues/265
- **[G30]** org.hl7.fhir.core #2503 «Loading R4 package in R5 context fails», 2026 — https://github.com/hapifhir/org.hl7.fhir.core/issues/2503
- **[G31]** org.hl7.fhir.core #2736: ошибки cross-version extensions — https://github.com/hapifhir/org.hl7.fhir.core/issues/2736
- **[G32]** FHIR/GoFSH #280: неверный перевод xver-расширений — https://github.com/FHIR/GoFSH/issues/280

**Тестовые данные и MCP**
- **[G33]** synthea #1236 «same seed gives different results», 1/1/22 — https://github.com/synthetichealth/synthea/issues/1236
- **[G34]** medplum #9616 «MCP search/fetch tools are dummy stubs», 0/0/7, 2026-06 — https://github.com/medplum/medplum/issues/9616

**CQL**
- **[G35]** cql-execution #266 «ELM TypeScript types», 0/0/21 — https://github.com/cqframework/cql-execution/issues/266
- **[G36]** cql-execution #297 «Performance issues with executor» — https://github.com/cqframework/cql-execution/issues/297
- **[G37]** clinical_quality_language #1768: npm-пакет без типов, 0/0/9, 2026 — https://github.com/cqframework/clinical_quality_language/issues/1768
- **[G38]** hapi-fhir #7222 «CQL $evaluate doesn't pick up updated Library», 0/0/27 (самый обсуждаемый открытый issue в hapi) — https://github.com/hapifhir/hapi-fhir/issues/7222

**Сигнал миграции: закрытые и заброшенные серверы**
- Azure API for FHIR выведен из эксплуатации 2026-09-30 — https://azure.microsoft.com/updates/azure-api-for-fhir-retirement
- FHIR Works on AWS: поддержка прекращена 2024-01-31 — https://github.com/aws-solutions/fhir-works-on-aws
- LinuxForHealth FHIR: последний push 2024-04, #4228 без ответа — https://github.com/LinuxForHealth/FHIR/issues/4228

### Стандарты и регуляторика

- **[S1]** FHIR package-list: R6 ballot4 2025-12-18, ballot5 2026-07-17, snapshot1 — https://hl7.org/fhir/package-list.json. Ballot-intro, где следующий баллот назван «certain», а даты публикации нет — http://hl7.org/fhir/6.0.0-ballot5/ballot-intro.html
- **[S2]** Diff R5→R6: новые коды CapStmt, `rest.resource.type` как uri, тип `resource` у search param, `OperationDefinition.synchronicity`, удалённые элементы — http://hl7.org/fhir/6.0.0-ballot5/diff.html. API Incubator (`$graph` и additional resources) — https://build.fhir.org/ig/HL7/api-incubator-ig/
- **[S3]** SVAP 2026: US Core 9 и USCDI v6 с 2026-08-29 — https://healthit.gov/blog/standards/advancements-in-health-it-oncs-2026-approved-svap-standards/
- **[S4]** IPS 2.0.1 (2026-06-19) — https://hl7.org/fhir/uv/ips/. SDC 4.0.0 (2026-03-24), CRMI 2.0.0 (2026-07-02) — по package-list HL7.
- **[S5]** CMS: API и IG для CMS-0057-F, срок 2027-01-01 — https://www.cms.gov/priorities/burden-reduction/overview/interoperability/implementation-guides-standards/application-programming-interfaces-apis-relevant-standards-implementation-guides-igs
- **[S6]** CMS-0062-P, proposed 2026-04-14 — https://www.federalregister.gov/documents/2026/04/14/2026-07205/medicare-and-medicaid-programs-patient-protection-and-affordable-care-act-interoperability-standards
- **[S7]** HTI-5 final 2026-10-26 — https://healthit.gov/resources/health-data-technology-and-interoperability-onc-deregulatory-actions-to-unleash-prosperity-hti-5-final-rule/. Отзыв HTI-2 2025-12-29 — https://www.aha.org/news/headline/2025-12-23-atsponc-proposes-deregulatory-actions-withdraws-hti-2-proposals
- **[S8]** SQL on FHIR v3 ballot, changes — https://github.com/FHIR/sql-on-fhir-v2/blob/master/input/pagecontent/changes.md
- **[S9]** Bulk Data STU3 и STU4-ballot, changes — https://github.com/HL7/bulk-data/blob/master/input/pagecontent/changes.md
- **[S10]** UDAP Security 2.0.0 — https://hl7.org/fhir/us/udap-security/
- **[S11]** TEFCA Facilitated FHIR SOP v2.0, действует с 2026-03-08 — https://rce.sequoiaproject.org/wp-content/uploads/2026/02/SOP-Facilitated-FHIR-Implementation-2.0-Draft-508.pdf. FAST в TEFCA — https://blog.hl7.org/fast-security-now-part-of-tefca-and-hti-2-requirements
- **[S12]** CMS Health Tech Ecosystem, Interoperability Framework: критерии 13–16 с 2026-07-04 (US Core, полный CapStmt, уведомления через Subscriptions) — https://www.cms.gov/initiatives/health-technology-ecosystem/overview/interoperability-framework
- **[S13]** Там же: требование уведомлений encounter/appointment через FHIR Subscriptions.
- **[S14]** Сроки EHDS по EEHRxF (2027-03-26, 2029, 2031) — https://www.frontiersin.org/journals/medicine/articles/10.3389/fmed.2025.1648170/full
- **[S15]** FHIRPath 3.0.0 STU, 2026-07-28 — http://hl7.org/fhirpath/STU3
- **[S16]** Треки HL7 Connectathon CAT42 (май 2026: SQL on FHIR, EHDS) и CAT43 (сентябрь 2026: AI Transparency с MCP/A2A, Testing/TestScript, Bulk testing, Subscriptions) — по страницам HL7 Confluence.
- **[S17]** AI Transparency on FHIR 1.0.0-ballot, 2025-12-09 — по package-list HL7. Блог HL7 про MCP и A2A — https://blog.hl7.org/building-the-standards-infrastructure-for-healthcare-ai-lessons-from-the-interoperability-journey
- **[S18]** PDex 2.2.0, `$bulk-member-match` — https://build.fhir.org/ig/HL7/davinci-epdx/changehistory.html
- **[S19]** HTI-4 final — https://healthit.gov/resources/hti-4-final-rule-electronic-prescribing-real-time-prescription-benefit-and-electronic-prior-authorization/
- **[S20]** HL7 AI Office (2025-07-08): официальной спецификации FHIR-MCP нет, все существующие серверы — community. Источник тот же, что в [S17].

### Инструменты и конкуренты

**Валидаторы**
- **[T1]** GoFHIR, сравнение старта и памяти (данные третьей стороны) — https://gofhir.github.io/validator/docs/getting-started/comparison
- **[T2]** Inferno June 2026 update (validator 1.0.78, «new failures») — https://inferno.healthit.gov/news/2026-06-june-update/
- **[T3]** Закрытие Inferno Resource Validator Service — https://inferno.healthit.gov/news/2026-07-planned-resource-validator-deprecation/
- **[T4]** HAPI: profile validation — самый медленный режим — https://groups.google.com/g/hapi-fhir/c/qVbaPVrQVIs

**Diff профилей**
- **[T5]** (архив, ≤2022) #IG creation «Comparison with previous versions» — https://chat-archive.fhir.org/stream/179252-IG-creation/topic/Comparison.20with.20previous.20versions.html
- **[T6]** (архив, ≤2022) #IG creation «Profile Diffing Tool» — https://chat-archive.fhir.org/stream/179252-IG-creation/topic/Profile.20Diffing.20Tool.html
- **[T7]** HealthSamurai fhir-profile-diff — https://github.com/HealthSamurai/fhir-profile-diff

**Тестовые данные**
- **[T8]** Synthea → профили MII не работают — https://medblocks.com/blog/highmed-synthea-german-mii-fhir-profiles
- **[T9]** Inferno uscore-data-script — https://github.com/inferno-framework/uscore-data-script
- **[T10]** SG-LLM, генерация по схеме с repair-циклом через валидатор — https://arxiv.org/pdf/2601.05847

**WASM**
- **[T11]** Kotlin FHIRPath с wasm-сборками — https://klibs.io/project/ohs-foundation/kotlin-fhirpath
- **[T12]** Браузерный fhir-resource-validator (поверхностный) — https://awesome.ecosyste.ms/projects/github.com%2Fjosephbartlett%2Ffhir-resource-validator

**Проверка CapabilityStatement**
- **[T13]** `$conforms` — «matter of research» — https://hl7.org/fhir/r4b/operation-capabilitystatement-conforms.html
- **[T14]** fhir-capability-analyzer (статический diff) — https://dnlbox.github.io/fhir-capability-analyzer/guide/compare.html

**Кросс-версия и FML**
- **[T15]** xver-пакеты 0.0.1-snapshot-2 — https://hl7.org/fhir/uv/xver-r5.r4/0.0.1-snapshot-2/downloads.html
- **[T16]** (архив, ≤2022) #javascript «Resource conversion»: JS-движок FML «worth writing» — https://chat-archive.fhir.org/stream/179169-javascript/topic/Resource.20conversion.html
- **[T17]** fmlrunner (KMP) — https://github.com/litlfred/fmlrunner

**Терминология**
- **[T18]** FHIRsmith (tx.fhir.org) — https://github.com/HealthIntersections/FHIRsmith
- **[T19]** Лицензия Ontoserver — https://ontoserver.csiro.au/docs/6/licenses.html
- **[T20]** Snowstorm Lite — https://github.com/IHTSDO/snowstorm-lite
- **[T21]** HL7 tx-ecosystem test cases — https://build.fhir.org/ig/HL7/fhir-tx-ecosystem-ig/branches/main/testcases.html

**Rust-конкуренты**
- **[T22]** ferrobridge-validator (BUSL) — https://docs.rs/crate/ferrobridge-validator/latest
- **[T23]** ThalamiQ tlq-fhir-core — https://github.com/thalamiq/tlq-fhir-core
- **[T24]** HeliosSoftware hfs — https://github.com/HeliosSoftware/hfs
- **[T25]** lib.rs: octofhir-fhirpath-compiler «82.7%» — https://lib.rs/crates/octofhir-fhirpath-compiler

**Редакторы**
- **[T26]** CQL-расширение VS Code требует Java — https://marketplace.visualstudio.com/items?itemName=cqframework.cql. cql-to-elm 4.0.0 (JS-бета) — https://github.com/cqframework/clinical_quality_language/releases/tag/v4.0.0
- **[T27]** @aehrc/fsh-language-server — https://www.pkgstats.com/pkg:@aehrc/fsh-language-server

**MCP**
- **[T28]** WSO2 fhir-mcp-server — https://pypi.org/project/fhir-mcp-server/
- **[T29]** Momentum FHIR MCP — https://www.themomentum.ai/blog/introducing-fhir-mcp-server-natural-language-interface-for-healthcare-data

**Aidbox (чего избегать)**
- **[T30]** Release notes (FHIR Schema-only, `$batch-validate`, Database page, Notebooks, `$materialize`, MCP, Forms) — https://www.health-samurai.io/docs/aidbox/overview/release-notes.md. Notebooks — https://www.health-samurai.io/docs/aidbox/overview/aidbox-ui/aidbox-notebooks. MCP — https://www.health-samurai.io/docs/aidbox/developer-experience/ai

**Наблюдаемость**
- **[T31]** Трассировка FHIR через OpenTelemetry без семантических конвенций — https://oneuptime.com/blog/post/2026-02-06-trace-hl7-fhir-api-requests-opentelemetry/view

---

## Ограничения ресерча

- Jira HL7 напрямую не сканировали. Сигналы оттуда пришли через changes.md спецификаций и Zulip.
- Реакции на GitHub по FHIR-репозиториям малы, поэтому сила спроса там оценена по комментариям и дублям.
- WEDI — единственный источник по версиям IG в IPPS FY2027. Там же, по вторичному источнику, сказано, что (g)(10) сохраняется в HTI-5. Оба утверждения стоит перепроверить по первоисточнику, прежде чем на них опираться.
- Длина Zulip-ссылок: темы с длинными названиями кодируются полностью, поэтому ссылки длинные. Хвост `near/<id>` открывает нужное сообщение.
