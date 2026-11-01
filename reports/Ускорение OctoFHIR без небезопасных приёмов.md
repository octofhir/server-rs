## Резюме

По опубликованным числам OctoFHIR уже обгоняет лидеров по пропускной способности во всех трёх наборах: CRUD 11812 против 5297 req/s, Import 7016 против 4269 res/s, Search 5803 против 3583 req/s. Это разные прогоны: OctoFHIR измерен 2026-09-10 в ветке `octofhir`, лидеры — 2026-10-30 в `main`, где OctoFHIR не участвовал ([SOURCE_DATA OctoFHIR 2026-09-10](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_2026-09-10T04:24:44Z.json), [SNAPSHOT 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SNAPSHOT_2026-10-30T00:03:50Z.json)). Проигрывает OctoFHIR только p99 поиска: 64,5–98,7 мс против 21,2–59,4 мс у Aidbox в тех же 24 группах.

Ровные ~65 мс — кумулятивный p99 k6, а не стоимость запросов: при 30 VU и 5803 req/s среднее время ответа не превышает 5,2 мс. Во всех 25 прогонах ветки `octofhir` Postgres стоял на лимите 8 CPU (измерено). Наиболее вероятная причина — CFS-троттлинг контейнера Postgres, которому OctoFHIR подаёт до 30 активных бэкендов (вывод; счётчики троттлинга не сняты).

Поэтому первыми идут два изменения конфигурации, не влияющие на результаты. Нужно ограничить одновременную работу в Postgres примерно 8–12 запросами и выключить JIT и параллельные планы для БД `octofhir`; ожидаемый p99 — 8–20 мс (вывод). Затем нужно снижать CPU Postgres на один поиск (~1,38 мс): при фиксированной квоте это единственный рычаг RPS.

Главный структурный дефект пути записи — глобальная advisory-блокировка. Каждая условная запись берёт исключительный замок на всю БД и останавливает все остальные записи сервера (`crates/octofhir-db-postgres/src/queries/crud.rs:40-53`, `crates/octofhir-db-postgres/sql/resource_write_gate.sql:6`). Следом идут стабильный текст SQL, однократная проверка ссылок, история без динамического SQL в триггерах и сборка образа с fat LTO и PGO.

Образ переопределяет профиль на thin LTO и 16 codegen units (`Dockerfile:66-68`). Это важно для CRUD, где приложение доходило до 771% CPU, и не важно для поиска, где приложение занято на 2 ядра из 8. Локально ничего не запускалось: каждый выигрыш ниже помечен как измеренный, выведенный или непроверенный и снабжён экспериментом.

Попутно найдены дефекты, которые стоит исправить независимо от скорости. `X-Skip-Validation` не привязан к роли, в репозитории `allow_skip_validation = true`, а `$import skipValidation` игнорирует переключатель. Без `_sort` нет `ORDER BY` при OFFSET-пагинации, `ext-1`, возможно, не проверяется, а `_total=estimate` ведёт себя как `none`.

## Диагностика поиска

Ровный p99 ~65 мс во всех 24 группах вызван очередью к Postgres, а не кодом OctoFHIR: во всех 25 прогонах ветки `octofhir` Postgres упирался в лимит 8 CPU. Ниже разобраны метрика, распределение CPU, механизм и исключённые причины. Статусы: «измерено» — опубликованные данные стенда, «вывод» — расчёт или рассуждение, «не проверено» — гипотеза без данных.

### Публикуемая «задержка» — это кумулятивный p99

Сценарий поиска — `constant-vus`, 30 VU, 2 минуты, без пауз, `discardResponseBodies: true` ([k6/search.js:18-32](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/k6/search.js#L18-L32)). Каждая итерация обходит 24 группы с `_count=20`, без `_total`, `_sort` и `_include`. Отчёт строится как `avg_over_time(k6_http_req_duration_p99)` и подписан «P99» ([generate-snapshot.js:198-202](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/scripts/generate-snapshot.js#L198-L202)).

Выходной плагин k6 копит все сэмплы с начала теста и никогда не очищает их, поэтому каждый `_p99` — перцентиль за весь прогон ([xk6-output-prometheus-remote](https://raw.githubusercontent.com/grafana/xk6-output-prometheus-remote/main/pkg/remotewrite/remotewrite.go)). Опубликованные значения 2026-09-10 совпадают со срезом в 04:50:00: у OctoFHIR это 64,9 мс, у Aidbox 84,4 мс в холодную первую минуту против 40 мс в установившемся режиме (измерено, [снимок Grafana 09-10](https://grafana.cdr.aidbox.dev/api/snapshots/NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3)). Почему выбирается именно этот срез, не проверено: шаг подзапроса VictoriaMetrics не сверялся с документацией.

Закон Литтла для замкнутого цикла даёт 30 / 5803 = 5,17 мс на запрос вместе с накладными k6 (вывод). Отношение p99 к среднему у OctoFHIR не меньше 12,5, у Aidbox 2,5–7. Такое отношение при плоском p99 во всех формах запроса указывает на бимодальное распределение: ~95% запросов за 2–3 мс и 3–5% в паузе 50–75 мс (вывод; проверка: 0,95·2,5 + 0,05·55 ≈ 5,1 мс).

Ряд p99 OctoFHIR плоский от первого сэмпла до последнего: 64,5–66,6 мс в каждой группе (измерено). Значит, дело не в прогреве. Групп по 24, на каждую ~240 req/s и ~290 запросов в хвосте 1%, поэтому любая глобальная пауза поднимает p99 всех групп одинаково (вывод).

### Postgres на лимите, приложение почти простаивает

| Фаза, прогон | Postgres CPU, % | Приложение CPU, % | p99 | Источник |
|---|---|---|---|---|
| Поиск OctoFHIR, 2026-09-10 | 800 (макс), 740–800 | 202–214 | 64,5–98,7 мс | [Grafana 09-10](https://grafana.cdr.aidbox.dev/api/snapshots/NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3) |
| Поиск Aidbox, 2026-09-10 | 734–776 | 734–748 | 36–123 мс (холодный срез) | там же |
| CRUD OctoFHIR, 2026-09-10 (11812 req/s) | 793 (макс) | 444 (среднее) | все операции 76–90 мс | там же |
| CRUD OctoFHIR, 2026-07-25 | 696 | 771 (макс) | чтение 18–24 мс | [SOURCE_DATA 07-25](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_2026-07-25T15:12:45Z.json), [Grafana 07-25](https://grafana.cdr.aidbox.dev/api/snapshots/jFvY2yVtRyi84FTblc6QLUNpjWGlKVoZ) |
| Поиск Medplum, 2026-10-30 | ~120–142 | 8 реплик на 84–100% от 1 CPU | 72,0–90,5 мс | [Grafana 10-30](https://grafana.cdr.aidbox.dev/dashboard/snapshot/kX4FlUTLki0a7iSoog4zzvFC20WQRqnV) |
| Поиск WSO2, 2026-10-30 | 800 (макс) | — | в основном 68–80 мс | там же |

Пол p99 не зависит от пропускной способности: 470 req/s дают 77 мс, 2498 — 71 мс, 4956 — 69 мс, 5765 и 5803 — 65 мс (измерено по 25 прогонам). Тот же плоский пол ~70–80 мс есть у WSO2 (Postgres на лимите) и Medplum (реплики на лимите 1 CPU). Его нет в CRUD 07-25, где приложение OctoFHIR дошло до 771%, а Postgres остался ниже лимита.

CPU Postgres на один поиск: OctoFHIR ≈ 8 / 5803 = 1,38 мс, Aidbox ≈ 7,7 / 3734 = 2,1 мс, Medplum ≈ 0,39 мс (вывод). У Medplum работа перенесена в приложение. Приложение OctoFHIR тратит ~0,35 мс на поиск, так что суммарно OctoFHIR дешевле всех на запрос, а система упирается только в квоту Postgres.

### Механизм: CFS-квота и 30 активных бэкендов

Postgres, Aidbox и OctoFHIR получают `cpus: '8'`, то есть CFS-квоту, а не cpuset ([docker-compose.yaml:23](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L23)). Пул OctoFHIR — 32 соединения при 30 VU ([docker-compose.yaml:164](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L164)). Период CFS по умолчанию — 100 мс; израсходовав квоту, группа ждёт следующего периода ([Linux CFS Bandwidth Control](https://docs.kernel.org/scheduler/sched-bwc.html)).

Контейнер, параллельность которого выше квоты, кратко занимает больше ядер, а потом стоит до ~95 мс, что «катастрофично для хвостовой задержки» ([Dan Luu, container throttling](https://danluu.com/cgroup-throttling/)). Спрос OctoFHIR оценивается в ~20 ядер при квоте 8 (вывод). Каждый период выглядит как всплеск 25–40 мс на многих ядрах и заморозка до границы периода; это даёт ~5% остановленных запросов и p99 60–75 мс (вывод, совпадает с наблюдаемым).

Приложение OctoFHIR так не страдает: tokio берёт число потоков из `available_parallelism`, которое учитывает cgroup-квоту ([std docs](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)). Postgres запускает процесс на соединение и может превысить квоту. У Aidbox большую часть времени запроса занимает JVM, поэтому одновременно активно меньше бэкендов и всплески короче (вывод).

Есть оговорка. Число ядер CI-хоста `hz-cdr` неизвестно; при ровно 8 ядрах троттлинга быть не может, и пол объяснялся бы обычной очередью при насыщении (не проверено). Ограничение конкуренции помогло бы и тогда, но слабее.

### Что исключено чтением кода

Путь поиска короткий, и ни один его шаг не даёт равномерных 60 мс:

| Кандидат | Почему исключён | Где |
|---|---|---|
| Auth и QuickJS | анонимный запрос получает общий `Arc`-контекст и сразу идёт дальше | `crates/octofhir-server/src/middleware.rs:936-957` |
| Аудит | выключен переменной `OCTOFHIR__AUDIT__ENABLED: 'false'` | [docker-compose.yaml:185](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L185) |
| `COUNT(*)` для `_total` | строится только при `_total=accurate` | `crates/octofhir-db-postgres/src/queries/search.rs:1235-1243` |
| Пинг пула | `test_before_acquire(false)` | `crates/octofhir-db-postgres/src/pool.rs:38` |
| Реестр параметров | снимок `ArcSwap`, O(1) | `crates/octofhir-search/src/registry.rs:104-111` |
| Сжатие | выключено по умолчанию | `crates/octofhir-server/src/config.rs:265` |
| Суммирование серий k6 | одна серия на группу, метка `status: '200'` | [Grafana 09-10](https://grafana.cdr.aidbox.dev/api/snapshots/NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3) |
| Прогрев | ряд плоский с первой минуты | там же |

На один поиск приходится один `fetch_all` без транзакции, `SET` и `statement_timeout`; строки идут как `resource::text` без разбора в `Value` (`crates/octofhir-db-postgres/src/queries/search.rs:1466-1510`). Два tracing-спана на запрос дешёвы и не дают пауз (вывод).

### Согласование с выводами о сборке и CRUD

Заметки о пути чтения правы: образ собран с thin LTO, 16 codegen units и `debug=0` вместо профиля репозитория с `lto = true` и `codegen-units = 1` (`Dockerfile:66-68`, `Cargo.toml` `[profile.release]`). Заметки о поиске тоже правы: на p99 поиска это не влияет, потому что приложение занято на 2 ядра из 8. Расхождение снимается разделением по наборам.

CRUD чередует узкие места. 07-25 приложение было на 771% при Postgres 696%, и p99 чтения держался на 18–24 мс: здесь LTO и PGO полезны. 09-10 Postgres был на 793%, и p99 всех операций CRUD прыгнул до плоских 76–90 мс: это тот же пол троттлинга, и ограничение конкуренции к Postgres поможет и CRUD.

Ещё одна деталь влияет на следующий прогон. Коммит 35fa56b перевёл поисковые запросы в `persistent(false)` уже после образа 09-10, поэтому каждый поиск теперь заново разбирается и планируется (`crates/octofhir-db-postgres/src/queries/search.rs:2273`, `:2402`). Это может поднять CPU Postgres на запрос; до публикации нового прогона это нужно измерить (не проверено).

### Гипотезы по убыванию вероятности

| # | Гипотеза | Вероятность | Подтверждение | Исправление | Эффект |
|---|---|---|---|---|---|
| H1 | CFS-троттлинг Postgres из-за ~30 активных бэкендов | очень высокая | `container_cpu_cfs_throttled_periods_total{name="perf-postgres"}` за окно поиска; локально `cpu.stat` до/после; свип пула 8–32; контроль с `cpuset: "0-7"` | лимитер ~8–12 одновременных запросов к Postgres | p99 ~65 → 8–20 мс (вывод), RPS прежний |
| H2 | Высокий CPU Postgres на поиск (1,38 мс) | высокая, как причина насыщения | `pg_stat_statements.track_planning = on`, `perf top` по бэкенду | меньше разбора/планирования и перепроверок | RPS ∝ 1/CPU на запрос (вывод) |
| H3 | JIT и параллельные планы на дорогих запросах | средняя; объясняет token 71–81 и quantity 85–99 мс | `_debug=search-explain-analyze`: секция `JIT:`, `Workers Launched` | `jit = off`, `max_parallel_workers_per_gather = 0` для БД | −6…35 мс у этих групп (вывод) |
| H4 | Артефакт измерения k6 | низкая для OctoFHIR, реальная для Aidbox 09-10 | ряды p95/p99 из VM, итог `k6 run --summary-trend-stats` | предложить апстриму итоговое значение | — |
| H5 | Нет `TCP_NODELAY`: Nagle и задержанный ACK 40 мс | низкая | A/B с `tap_io(set_nodelay)`, `tcpdump` на паузы ~40 мс | `set_nodelay(true)` | от нуля до малого |
| H6 | Накладные приложения | очень низкая | samply, tokio-console | — | — |

Конфигурация Postgres стенда подтверждает H3: `max_parallel_workers_per_gather = 4`, а `jit` не задан и остаётся по умолчанию ([postgres.conf:5-10](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/infra/postgres/postgres.conf#L5-L10)). OctoFHIR не меняет эти параметры в продуктовом коде; их выключают только тесты (`crates/octofhir-db-postgres/src/queries/search/revinclude_tests.rs:111`).

## Ранжированная таблица техник

Ниже 24 техники, упорядоченные по влиянию на публичный бенчмарк с поправкой на уверенность и стоимость; первые три меняют только поиск и не меняют результаты. Статус выигрыша: И — измерено (данные стенда, замеры в репозиториях или внешний бенчмарк), В — вывод, Н — не проверено. Усилия: S — дни, M — 1–2 недели, L — больше.

| # | Техника | Область | Ожидаемый выигрыш (статус, источник) | Усилия | Риск | Как проверить на OctoFHIR | Строка бенчмарка |
|---|---|---|---|---|---|---|---|
| 1 | Лимитер ~8–12 одновременных запросов к Postgres (отдельный для поиска) | пул, поиск | p99 ~65 → 8–20 мс (В, [CFS](https://docs.kernel.org/scheduler/sched-bwc.html)); пул 2048→96 дал 100→2 мс у Oracle (И, [HikariCP](https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing)) | S | низкий при отдельном лимитере | `cpu.stat` Postgres, свип 8/10/12/16/32 | Search p99, все 24 группы; CRUD p99 |
| 2 | `jit = off`, `max_parallel_workers_per_gather = 0` для БД `octofhir` | поиск | −6…35 мс у token и quantity (В) | S | низкий | `_debug=search-explain-analyze` без `JIT:` и `Workers` | Search token, quantity combo/component |
| 3 | Снизить CPU Postgres на поиск: учёт планирования, именованные запросы с custom-планами, аудит 24 планов | поиск SQL | RPS ∝ 1/CPU; −30% CPU ≈ +30% RPS (В) | M | низкий при сверке результатов | `pg_stat_statements` с `track_planning`, `perf top` | Search RPS |
| 4 | Заменить глобальную advisory-блокировку ключами по типу и критерию | запись | снимает сериализацию всей БД при условных записях (Н для стенда) | M | средний: нужен тест гонок | счётчик `lock_conditional_writes`, ожидания `advisory` | Import, CRUD |
| 5 | Связывать txid и версию If-Match параметрами, а не литералами | запись | −1 RT на промах кэша и −1 RT на вытеснение (И, код sqlx) | S | низкий | число `queryid` в `pg_stat_statements` | Import; CRUD update |
| 6 | Проверять ссылки один раз и одним запросом | запись, валидация | −(N+1) RT на запись с N типами ссылок (В, код) | M | низкий | `log_statement=all`, стейтменты на POST | CRUD create; Import |
| 7 | История и метаданные без динамического SQL и row-триггера | запись | row 3816 мс против statement 642 мс на массовом UPDATE (И, [Cybertec](https://www.cybertec-postgresql.com/en/rules-or-triggers-to-log-bulk-updates/)); single-row Н | M | средний: все пишущие пути | pgbench, 1-строчный UPDATE, 3 варианта | CRUD update/delete |
| 8 | Fat LTO + CGU=1 в образе, затем PGO | сборка | LTO 10–20%, PGO ≥10% (И, [perf-book](https://nnethercote.github.io/perf-book/build-configuration.html)) | S/M | низкий | k6 CRUD, CPU приложения на запрос | CRUD (когда упирается приложение), Import |
| 9 | Валидация без глубоких копий, лишних обходов и с кэшем схем ≥2048 | валидация | Н; 1,4 мкс на инвариант (И, бенч fhirpath-rs) | M | низкий: все проверки сохраняются | `validation_bench`, FHIR262 | CRUD, Import |
| 10 | Сузить document GIN, даты на btree min/max, вернуть HOT | индексы | Patient 1,6k/s против 7,5k/s при 4 параметрах (Н, внутренний бриф); Medplum Import ×3,6 после сокращения SP (И) | M | средний: планы поиска | `n_tup_hot_upd`, сверка результатов | CRUD, Import |
| 11 | Ответ без копий: `Bytes`, Bundle байтами, ленивый разбор; облегчённый middleware, `TCP_NODELAY` | HTTP, чтение | 1 alloc+memcpy на чтение, 1 проход JSON на запись Bundle (В) | S–M | низкий | samply, сравнение тел ответов | CRUD read; CPU приложения в Search |
| 12 | WAL: `wal_compression`, `max_wal_size` 32–64 ГБ, `commit_delay` | Postgres | компрессия выигрывает у её отсутствия по TPS (И, [EDB](https://enterprisedb.com/blog/you-can-now-pick-your-favorite-compression-algorithm-your-wals)) | S | низкий, синхронно | `pg_stat_wal`, `pg_stat_checkpointer` | Import, CRUD (свои развёртывания) |
| 13 | Auth без обращений к БД; QuickJS компилирует политику один раз | auth | 2–3 → 0 RT на запрос (В, код) | S–M | низкий при инвалидации | вызовы на запрос, `quickjs_bench` | вне стенда (анонимный доступ) |
| 14 | `ORDER BY id` по умолчанию, keyset-курсор, лимит `_offset`; `_revinclude`/`_has` через GIN | поиск | O(страница) вместо O(offset) (И, [use-the-index-luke](https://use-the-index-luke.com/no-offset)) + корректность | S–M | средний: риск плана `ORDER BY + LIMIT` | листание при вставках, p99 24 групп | вне стенда |
| 15 | Устойчивые планы без `vacuumdb`: фоновый `ANALYZE` после импорта, `CREATE STATISTICS` | планы | оценки без стоимости записи (И, [CREATE STATISTICS](https://www.postgresql.org/docs/18/sql-createstatistics.html)) | S | низкий | планы 24 групп с `vacuumdb` и без | Search в прогоне `main` |
| 16 | Кэш вердикта targetProfile по `(id, versionId, profile, generation)` | валидация | Н | M | низкий: версия неизменна | счётчик рекурсивных валидаций | CRUD, Import (стенд включает targetProfile) |
| 17 | Бинарный COPY для create-only записей бандла | Import | мал при ~1,6k строк: COPY слаб на 1000 строк (И, [Tiger Data](https://www.tigerdata.com/blog/benchmarking-postgresql-batch-ingest)) | M | средний | resources/s, байты WAL | Import |
| 18 | A/B аллокатора: jemalloc+THP против mimalloc | runtime | ~5% времени и +15% памяти у rustc (И, [Kobzol](https://kobzol.github.io/rust/rustc/2023/10/21/make-rust-compiler-5percent-faster.html)) | S | низкий | RPS, p99, RSS | CRUD |
| 19 | Кэш раскрытий ValueSet как `HashSet`, предраскрытие required-биндингов | терминология | Н | M | низкий | счётчик промахов | CRUD, Import (холодный кэш) |
| 20 | Опциональный btree для `_sort` по дате; `_total=estimate` через `EXPLAIN` | поиск | Н | S–M | низкий | `EXPLAIN` с `_sort=-date` | вне стенда |
| 21 | Сохранять скомпилированные реестры по хэшу набора пакетов | старт | Н; самый медленный future 2,7 с (комментарий в коде) | M | низкий | `elapsed_ms` в логах старта | старт |
| 22 | Отдельный семафор для импорта бандлов | пул | изоляция CRUD от импорта (В) | S | низкий | p99 CRUD при параллельном импорте | вне стенда (фазы последовательны) |
| 23 | JWT на `aws_lc_rs`, дедупликация зависимостей | зависимости | Н | S–M | средний | criterion `JwtService::decode` | вне стенда |
| 24 | `io_method = io_uring` в своих развёртываниях PG18 | Postgres | холодный `COUNT(*)` 15,8 → 5,7 с (И, [pganalyze](https://pganalyze.com/blog/postgres-18-async-io)) | S | низкий | поиск на холодном кэше | вне стенда (тёплый кэш) |

## Топ-15: описания и эксперименты

Каждая из 15 техник ниже описана тремя частями: суть, доказательства со статусом и локальный эксперимент с критерием успеха. Эксперименты предполагают Linux-хост с более чем 8 ядрами и стенд ветки `octofhir` (см. «Методика»).

### 1. Ограничить одновременную нагрузку на Postgres

Сейчас пул — 32 соединения при 30 VU, и все 30 бэкендов могут быть активны на квоте 8 CPU ([docker-compose.yaml:164](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L164)). Предлагается семафор в слое хранения, отдельный для поиска, размером ~1–1,5 ядра Postgres. Пул целиком уменьшать не стоит: CRUD и импорт делят его, а длинные транзакции бандлов могут занять маленький пул.

Пол p99 не зависит от RPS в 25 прогонах (И), формула вики PostgreSQL даёт (2 × 8) + 0 ≈ 16 активных соединений ([PostgreSQL wiki](https://wiki.postgresql.org/wiki/Number_Of_Database_Connections)), HikariCP показывает падение времени ответа с ~100 до ~2 мс при уменьшении пула ([HikariCP](https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing)). Ожидание: p99 8–20 мс за счёт честной FIFO-очереди sqlx вместо заморозок (В). RPS останется около 5,8k, потому что упирается в квоту.

Эксперимент: поднять compose ветки `octofhir`, наполнить данные фазами CRUD и Import, снять `docker exec perf-postgres cat /sys/fs/cgroup/cpu.stat` до и после `k6 run k6/search.js`. Повторить для лимита 8, 10, 12, 16, 32 и для контроля с `cpuset: "0-7"` вместо `cpus: '8'`. Успех: `nr_throttled` падает на порядок, p99 каждой группы ниже Aidbox в той же группе (21–59 мс), RPS не ниже 5800, CRUD при 300 VU не теряет больше 3%.

### 2. Выключить JIT и параллельные планы для API-запросов

На стенде `max_parallel_workers_per_gather = 4`, `jit` по умолчанию, а OctoFHIR ничего не переопределяет (И, [postgres.conf:5-10](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/infra/postgres/postgres.conf#L5-L10)). Группы выше пола 65 мс — широкие token (71–81 мс) и quantity combo/component (85 и 99 мс). Параллельные воркеры добавляют процессы и быстрее сжигают квоту, JIT дорог на компиляцию (В); в PostgreSQL 19 RC1 JIT уже выключен по умолчанию ([PG19 release notes](https://www.postgresql.org/docs/19/release-19.html)).

Изменение: `ALTER DATABASE octofhir SET jit = off` и `SET max_parallel_workers_per_gather = 0` при бутстрапе или в `after_connect` пула. Для тяжёлых аналитических путей (SQL on FHIR, экспорт) параллельность можно вернуть через `SET LOCAL`. Результаты запросов не меняются.

Эксперимент: `GET /fhir/Encounter?status=finished,in-progress&_count=20&_debug=search-explain-analyze` и `GET /fhir/Observation?component-value-quantity=gt80&_count=20&_debug=search-explain-analyze`, искать `JIT:` и `Workers Launched`. Затем A/B k6 search с p99 по группам и CPU Postgres. Успех: токен- и quantity-группы опускаются к общему полу, CPU Postgres на запрос не растёт.

### 3. Снизить CPU Postgres на один поиск

При фиксированной квоте 8 CPU RPS обратно пропорционален CPU Postgres на запрос (В). Сейчас это ~1,38 мс, против ~2,1 мс у Aidbox (В). Кандидаты: разбор и планирование на каждом выполнении из-за `persistent(false)` (`crates/octofhir-db-postgres/src/queries/search.rs:2273-2402`), `jsonb_out` для 21 строки, перепроверки после GIN и триграмм.

Литералы встраиваются в SQL ради удобства рендера: границы quantity и JSONPath (`crates/octofhir-db-postgres/src/queries/search.rs:1245-1249`). Generic-план опасен: плотность значений в FHIR сильно разная, поэтому кастомные планы нужно сохранить ([PREPARE](https://www.postgresql.org/docs/18/sql-prepare.html)). Компромисс для проверки — параметры плюс именованный запрос с `plan_cache_mode = force_custom_plan`: экономится разбор, план остаётся под значение (В).

Эксперимент: включить `pg_stat_statements.track_planning` (расширение уже в `shared_preload_libraries` стенда), сбросить статистику, прогнать 2 минуты поиска. Сравнить `mean_plan_time` и `mean_exec_time` по формам запросов, снять `perf top -p <pid бэкенда>` для разбивки на `jsonb_out`, планировщик и jsonpath. Успех: CPU Postgres на запрос −20% при побитово равных множествах `id` во всех 24 группах.

### 4. Убрать глобальную блокировку условных записей

Это самый серьёзный структурный дефект записи. Все записи используют один ключ `(1330006098, 1)`: обычные операторы берут его разделяемо через statement-триггер на каждой таблице (`crates/octofhir-db-postgres/sql/resource_write_gate.sql:6`, `crates/octofhir-db-postgres/src/schema.rs:256-261`), а условные операции — исключительно (`crates/octofhir-db-postgres/src/queries/crud.rs:40-53`). Исключительный замок берут одиночные условные записи (`crates/octofhir-server/src/write.rs:1007`) и бандлы с условными записями на всё время записи (`crates/octofhir-server/src/handlers.rs:4994`, `:5604-5612`, `:7542`).

Следствие: один If-None-Exist ждёт все идущие записи, а затем блокирует все новые записи во всей БД до коммита. Влияние на стенд не проверено: патч-бандлы Synthea используют условные ссылки, которые по замыслу берут замок только при `conditional_ref_dependent` (`crates/octofhir-server/src/handlers.rs:5602-5607`). Для реальной загрузки US Core с `ifNoneExist` это главная точка сериализации (В).

Исправление по шагам. Ключ по типу ресурса: запись берёт разделяемый ключ своих типов, условная операция — исключительный ключ искомого типа; условия с цепочками и `_has` берут отсортированный набор типов из IR или глобальный ключ. Для формы `identifier=system|value` возможен ключ по хэшу критерия или таблица `conditional_key` с уникальным индексом; затем можно убрать per-statement plpgsql-триггер.

Эксперимент: счётчик вызовов `lock_conditional_writes` и сэмплирование `pg_stat_activity` (`wait_event_type = 'Lock' AND wait_event = 'advisory'`) каждые 100 мс во время k6 import. Затем `k6/benchmarks/transaction-upsert.js` параллельно с `crud.js` на текущем коде и на прототипе за флагом. Обязательный тест корректности: 50 параллельных одинаковых If-None-Exist создают ровно 1 ресурс, отдельно для условий с цепочкой.

### 5. Сделать текст SQL стабильным

Вставка бандла встраивает txid литералом (`crates/octofhir-db-postgres/src/queries/crud.rs:1248-1300`), обновление с If-Match встраивает версию (`crates/octofhir-db-postgres/src/queries/crud.rs:836-841`). Каждый новый текст — промах кэша sqlx: отдельный круг Parse/Describe/Sync, а после 100 операторов ещё круг Close/Sync на вытеснение (И, `sqlx-postgres-0.9.0/src/connection/executor.rs:23-80`, `:184-195`; ёмкость 100 по умолчанию — [PgConnectOptions](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html)). Заодно засоряется `pg_stat_statements`.

Исправление: `$n::bigint` вместо литерала; при необходимости поднять `statement_cache_capacity`, если горячих операторов больше 100 на ~150 типов. План `INSERT … SELECT FROM UNNEST` и проверка `h.txid >= $n` по PK `(id, txid)` от значения txid не зависят (В).

Эксперимент: после импорта посчитать `count(DISTINCT queryid)` для вставок по каждому типу и `pg_prepared_statements` на соединение. Затем k6 import (resources/s) и CRUD update (p99). Успех: один `queryid` на тип и операцию, импорт не медленнее.

### 6. Проверять ссылки один раз

Сейчас одна запись разрешает ссылки 2–3 раза. Валидатор читает полные тела `read_many` последовательно по типам (`crates/octofhir-server/src/reference_resolver.rs:292-325`), быстрый путь повторяет проверку существования (`crates/octofhir-server/src/handlers.rs:1015-1035`, `:1183`), подготовленный путь перечитывает снимки и сравнивает их глубоко (`crates/octofhir-server/src/write.rs:423-466`). Для N типов ссылок это до N последовательных чтений и ещё один круг (В).

Исправление: в фазе 4 валидатора использовать `exists_many_grouped` — один `UNION ALL` по `id = ANY($n)` (`crates/octofhir-db-postgres/src/queries/crud.rs:280-318`). Результат передать в шаг записи как `known_refs`, тела читать только для targetProfile или политики. Проверку можно встроить в сам `INSERT` через CTE с условием на число найденных ссылок; кэш «ресурс существует» между запросами без инвалидации на удаление вводить нельзя.

OctoFHIR строже HAPI и Medplum: проверка ссылок у него включена по умолчанию, у них — опциональна ([HAPI JPA configuration](https://hapifhir.io/hapi-fhir/docs/server_jpa/configuration.html), [Medplum Project.d.ts](https://app.unpkg.com/@medplum/fhirtypes@4.3.10/files/dist/Project.d.ts)). Эксперимент: `log_statement=all`, POST Observation с 1, 3 и 6 типами ссылок, счёт операторов на запрос. Успех: не больше двух операторов на создание при любом N, p99 create в k6 CRUD снижается.

### 7. История и метаданные без динамических триггеров

Одиночный UPDATE вызывает три триггера: разделяемый замок (plpgsql на оператор), BEFORE UPDATE на строку для метаданных и AFTER-триггер истории (`crates/octofhir-db-postgres/src/schema.rs:256-279`, `:309-325`). Триггер истории строит SQL через `EXECUTE format(...)`, который разбирается и планируется при каждом срабатывании (`crates/octofhir-db-postgres/sql/resource_history.sql:3-34`). Цена переходных таблиц для одной строки нигде не опубликована (Н).

Массовые замеры: row-триггер 3816 мс, statement-триггер 642 мс, без триггера 295 мс ([Cybertec](https://www.cybertec-postgresql.com/en/rules-or-triggers-to-log-bulk-updates/)); переходные таблицы в 7 раз быстрее row-триггера ([Tiger Data](https://www.tigerdata.com/blog/speed-up-triggers-by-7x-with-transition-tables)). Низкорисковый шаг — статичные функции на таблицу, генерируемые `SchemaManager`, чтобы plpgsql кешировал планы. Следующий — запись истории в том же операторе через data-modifying CTE и простановка meta в `SET`, как уже делает создание (`crates/octofhir-db-postgres/src/queries/crud.rs:110-122`).

Эксперимент: pgbench со скриптом на 1-строчный UPDATE таблицы с JSONB 4 КБ в трёх вариантах: текущие триггеры, статичная функция, CTE без триггеров. Записать TPS и байты WAL на операцию, затем p99 update/delete в k6 CRUD. Успех: +15% TPS при построчно равных таблицах истории после одинакового сценария.

### 8. Собрать образ с fat LTO, CGU=1 и PGO

Профиль репозитория уже правильный: `lto = true`, `codegen-units = 1`, `panic = "abort"`. Dockerfile переопределяет его на thin LTO и 16 CGU (`Dockerfile:66-68`), а стенд тянет `ghcr.io/octofhir/octofhir-server:latest` ([docker-compose.yaml:144](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L144)). Rust perf book оценивает LTO в 10–20% и PGO в 10% и больше ([perf-book](https://nnethercote.github.io/perf-book/build-configuration.html)).

Это важно там, где упирается приложение: CRUD 07-25 при 771% CPU приложения. Для поиска выигрыша не будет, пока узкое место — Postgres. cargo-pgo предупреждает, что нерепрезентативный профиль может замедлить бинарь; нагрузка k6 стенда репрезентативна по определению ([cargo-pgo](https://github.com/Kobzol/cargo-pgo)).

Эксперимент: собрать два образа, текущий и с профилем репозитория, после исправления №1. Прогнать `crud.js` 300 VU × 4 минуты и сравнить RPS и CPU приложения на запрос. Затем `cargo pgo build`, прогнать prewarm, CRUD, Import и Search против инструментированного бинаря, `cargo pgo optimize`; успех — CPU приложения на запрос −10% и больше.

### 9. Валидация без копий и лишних обходов

На запись приходятся две глубокие копии JSON (`crates/octofhir-server/src/operations/validation_reference.rs:133`, `../fhirschema/octofhir-fhirschema/src/validation/mod.rs:1185-1189`). Дальше идёт полное преобразование в `FhirNode`, которое кладётся в кэш 32 МиБ с ключом по адресу `Arc` и при записях никогда не попадает (`../fhirpath-rs/crates/octofhir-fhirpath/src/evaluator/prepared.rs:14-27`). Добавляются 3–4 полных обхода дерева, `format!` пути на каждом узле и кэш скомпилированных схем на 500 записей при ~655 схемах R4 (`crates/octofhir-server/src/server.rs:672-680`).

Измерено ранее: подготовленное вычисление инвариантов ~1,4 мкс на вызов, 4,3 мс на 1000 узлов против 13,0 мс в старом пути (`../fhirpath-rs/docs/EVALUATOR_PERFORMANCE.md:40-67`). `$validate` вырос с ~1590 до ~4800 RPS (`k6/benchmarks/VALIDATION_RESULTS.md`). Доля валидации во времени записи не измерена (Н).

Исправление: передавать `Arc<Value>`, пропускать `PreparedRoots` для одноразовых корней, слить обходы, на этапе компиляции пометить поддеревья без error-ограничений и required-биндингов и пропускать их. Пути строить лениво, кэш схем поднять до ≥2048, мемоизировать биндинги между схемами. `ele-1` уже проверяется структурно (`../fhirschema/octofhir-fhirschema/src/validation/mod.rs:1721-1740`); `ext-1` стоит добавить так же (см. «Отвергнутые варианты»).

Эксперимент: `cargo bench -p octofhir-fhirschema --bench validation_bench` до и после каждого шага. Долю валидации измерить k6 CRUD с `X-Skip-Validation: true` и без него, только на локальном стенде. Успех: CPU валидации Observation −30% (цель, В) при FHIR262 не ниже 98,7%.

### 10. Меньше индексов на запись, больше HOT

Whole-document GIN `jsonb_path_ops` создаётся на каждой таблице, если `document_gin_resource_types` не задан (`crates/octofhir-db-postgres/src/schema.rs:335-347`, `octofhir.toml:38-46`); стенд его не сужает. Даты индексируются GiST по `tstzrange` (`crates/octofhir-db-postgres/src/functional_indexes.rs:277-290`). Любой индекс по `resource` делает каждый UPDATE не-HOT, потому что каждая запись меняет `meta` (`crates/octofhir-db-postgres/src/schema.rs:211-215`, [HOT](https://www.postgresql.org/docs/current/storage-hot.html)).

Внутренний бриф даёт ~10–14 мкс на строку для GiST против ~1,4 мкс для btree и 1,6k против 7,5k записей Patient в секунду (Н, источник не опубликован). Внешний натурный эксперимент: Import Medplum вырос с ~850 до ~3063 res/s после сокращения индексируемых SP до 16 ([коммит 8b71f14](https://github.com/HealthSamurai/fhir-server-performance-benchmark/commit/8b71f14)). WSO2 оценивает GIN по документу в ~2,4× к цене записи ([wso2 DESIGN.md](https://github.com/wso2/fhir-server/blob/main/DESIGN.md)).

Свойство «новый SearchParameter без reindex» сохраняется: индексы — только ускорители с точной перепроверкой (`crates/octofhir-db-postgres/src/functional_indexes.rs:1-6`). Без document GIN неиндексированный параметр становится seq scan — результат верный, но медленный. Для однозначных дат подойдут два btree по `fhir_extract_date_min/max`, для Period и множественных значений GiST нужно оставить.

Эксперимент: 100k вставок Synthea Patient и Observation с каждым индексом по отдельности, мкс на строку и `pg_relation_size`. `EXPLAIN (ANALYZE, BUFFERS)` для `birthdate=ge1980` и `date=lt2015` с парой btree и с GiST при сверке множеств результатов. Доля HOT `n_tup_hot_upd / n_tup_upd` на Patient без GIN; успех — рост CRUD и Import при неизменных планах 24 групп.

### 11. Ответ без копий и лёгкий middleware

Каждое чтение копирует тело ресурса: `Body::from(stored.resource_json.clone())` (`crates/octofhir-server/src/handlers.rs:1609`). Поиск валидирует каждую запись через `RawValue::from_string` (`crates/octofhir-api/src/lib.rs:25-28`), а `Json` axum начинает буфер со 128 байт ([axum json.rs](https://raw.githubusercontent.com/tokio-rs/axum/main/axum/src/json.rs)). При наличии `AuthorizedRead` тело разбирается в `Value` целиком (`crates/octofhir-server/src/handlers.rs:1575-1583`); попадает ли стенд на этот путь, не проверено.

Middleware на запрос клонирует метод и URI, форматирует `%uri` в info-спан и строит метки метрик из `String` (`crates/octofhir-server/src/middleware.rs:550-629`, `crates/octofhir-server/src/metrics.rs:80-105`). `TCP_NODELAY` не выставлен (`crates/octofhir-server/src/server.rs:2449`), хотя пример axum делает это через `tap_io` ([axum ListenerExt](https://docs.rs/axum/latest/axum/serve/trait.ListenerExt.html)). Выигрыш мал и не измерен; он нужен для CRUD в фазах, где упирается приложение.

Исправление без `unsafe`: `Bytes` в `RawStoredResource` (`crates/octofhir-storage/src/types.rs:51`), Bundle собирается байтами в заранее выделенный буфер из доверенного вывода `jsonb_out`, разбор тела — только если политике нужен контент. Плюс кешированные дескрипторы метрик и `set_nodelay(true)`. Эксперимент: samply на профиле `profiling` при k6 read и search, `tcpdump` на паузы ~40 мс; успех — CPU приложения на чтение −10% при побайтно равных телах ответов.

### 12. WAL и контрольные точки

Конфигурация Postgres стенда не задаёт `wal_compression`; там `max_wal_size = 8GB`, `checkpoint_timeout = 15min`, `wal_buffers = 256MB` ([postgres.conf](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/infra/postgres/postgres.conf)). Первое изменение страницы после checkpoint пишет её целиком ([WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)), а история дублирует предыдущий JSONB при каждом обновлении. В тесте EDB любая компрессия WAL дала больше TPS, чем её отсутствие ([EDB](https://enterprisedb.com/blog/you-can-now-pick-your-favorite-compression-algorithm-your-wals)).

`commit_delay` остаётся синхронной фиксацией и не теряет подтверждённых записей ([Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)). Конфигурация Postgres на публичном стенде общая и принадлежит HealthSamurai, поэтому этот пункт — для собственных развёртываний и локальных замеров. Для Import 20 000 фиксаций за прогон незначимы, важны байты WAL и FPI; для CRUD важна частота сбросов (В).

Эксперимент: снимки `pg_stat_wal` (`wal_records`, `wal_fpi`, `wal_bytes`, `wal_buffers_full`) и `pg_stat_checkpointer` вокруг импорта. По одному параметру: `wal_compression` off/lz4/zstd, `max_wal_size` 8/32/64 ГБ, `commit_delay` 0 и ½ времени `pg_test_fsync` при `commit_siblings` 5 и 10. Успех: байты WAL на ресурс −20%, импорт +5%, `checkpoints_req` около нуля.

### 13. Auth без обращений к БД и QuickJS с компиляцией один раз

Даже при попадании в кэш JWT каждый запрос делает 2–3 обращения к Postgres: отзыв токена, клиент и пользователь (`crates/octofhir-server/src/middleware.rs:408`, `crates/octofhir-auth/src/middleware/auth.rs:132-160`). Запросы отзыва и клиента фильтруют по `resource->>'jti'` и `resource->>'clientId'` без индекса (`crates/octofhir-auth-postgres/src/revoked_token.rs:76-89`, `crates/octofhir-auth-postgres/src/client.rs:94-106`). Стоимость seq scan растёт с числом отзывов.

QuickJS на каждый вызов сериализует контекст в исходный текст JS, заново исполняет прелюдию хелперов и компилирует политику (`crates/octofhir-auth/src/policy/quickjs.rs:142-256`). В поиске с политикой, зависящей от ресурса, это повторяется для каждой строки (`crates/octofhir-server/src/authorization/snapshots.rs:136-170`). Комментарий «скрипты компилируются один раз» не соответствует коду (`crates/octofhir-auth/src/policy/engine.rs:255`).

На стенде HealthSamurai доступ анонимный, поэтому этот пункт не двигает публичные числа, но ускоряет реальный путь. Исправление: уникальные индексы по выражениям, зеркало отзывов в памяти процесса (сервер одноэкземплярный), moka-кэш клиентов и пользователей с инвалидацией на запись. Для QuickJS: прелюдия один раз на рантайм, политика — функция в кэше по поколению политик, контекст через `JSON.parse`, выбор рантайма через `try_lock`.

Эксперимент: `just bench-crud` до и после, вызовы `pg_stat_statements` на запрос, `cargo bench -p octofhir-auth --bench quickjs_bench`. Обязательная проверка: отзыв токена действует на следующем же запросе (401). Успех: ноль auth-запросов к БД в установившемся режиме.

### 14. Пагинация и связи: корректно и по индексу

Без `_sort` SQL не содержит `ORDER BY` (`crates/octofhir-search/src/sql_builder.rs:1243-1251`), а страницы идут через `LIMIT … OFFSET` (`crates/octofhir-search/src/sql_builder.rs:1272-1285`). Порядок строк в Postgres без `ORDER BY` не гарантирован, поэтому соседние страницы могут дублировать или пропускать ресурсы. `_offset` — неограниченный u32 (`crates/octofhir-search/src/params_converter.rs:1277-1281`).

`_revinclude`, `_has` и цепочки разворачивают ссылки через `jsonb_array_elements` на каждой строке и не используют GIN по `fhir_extract_ref` (`crates/octofhir-db-postgres/src/queries/search.rs:1691-1724`, `crates/octofhir-search/src/ir/chain.rs:156-229`). Keyset-пагинация работает за O(страница) и устойчива к вставкам ([use-the-index-luke](https://use-the-index-luke.com/no-offset)). Medplum ограничивает `_offset` 10 000 и предлагает курсор ([Medplum paginated search](https://www.medplum.com/docs/search/paginated-search)).

`ORDER BY id` несёт риск для стенда: при широком фильтре и `LIMIT 21` планировщик может пойти по PK и отбрасывать строки ([pganalyze](https://pganalyze.com/blog/5mins-postgres-planner-order-by-limit)). Поэтому изменение нужно мерить на всех 24 группах. Эксперимент: листать `Observation?_count=50` при параллельных вставках и искать дубли и пропуски; сравнить p99 24 групп с `ORDER BY id` и без, задержку страницы 100 для OFFSET и keyset.

### 15. Устойчивые планы без `vacuumdb` после импорта

С 2026-10-28 конвейер `main` не запускает `vacuumdb --analyze` перед поиском ([коммит e6a4941](https://github.com/HealthSamurai/fhir-server-performance-benchmark/commit/e6a4941)). Ветка `octofhir` его ещё запускает ([.drone.yml:179](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/.drone.yml#L179)). В тот же день поиск WSO2 упал с ~3,6–4,3k до ~1,9–2,3k req/s (И); связь со снятием `vacuumdb` не проверена.

OctoFHIR задаёт таблицам `autovacuum_analyze_scale_factor = 0.01` (`crates/octofhir-db-postgres/src/schema.rs:245-251`), но автовакуум может отставать сразу после импорта (В). Сервер может сам ставить фоновый `ANALYZE` на таблицы с большим `n_mod_since_analyze` после крупных бандлов. `CREATE STATISTICS` на выражения извлечения даёт оценки без стоимости записи ([CREATE STATISTICS](https://www.postgresql.org/docs/18/sql-createstatistics.html)); после `pg_upgrade` расширенная статистика не сохраняется ([PG18 release notes](https://www.postgresql.org/docs/18/release-18.html)).

Эксперимент: прогнать конвейер ветки `octofhir` без шага `vacuumdb` и сравнить планы 24 групп через `_debug=search-explain-analyze` и p99 с базовым прогоном. Затем включить фоновый `ANALYZE`. Успех: через 60 с после импорта планы совпадают с планами после `vacuumdb`.

## Конкуренты

В прогоне 2026-10-30 лидерство разделено между двумя серверами: Aidbox первый в CRUD (5297 req/s) и Import (4269 res/s), Medplum — в Search (3583 req/s). Поисковое число Medplum, вероятно, примерно на 40% состоит из ответов HTTP 400 (вывод, не подтверждено запуском).

| Сервер | CRUD, req/s | Import, res/s | Search, req/s | p99 поиска, мс | Узкое место в поиске | Ключевые особенности на стенде |
|---|---|---|---|---|---|---|
| Aidbox | 5297 | 4269 | 3392 | 21,2–59,4 | Postgres ~785–788% | образ `edge`, индексы под тест, `_total=none`, валидация включена |
| Medplum | 3048 | 2734 | 3583 | 72,0–90,5 | 8 реплик × 1 CPU; Postgres ~120–142% | 16 SP, 128 соединений, Redis, токен OAuth |
| WSO2 (Go) | 3873 | 3393 | 1878 | 68–80, выбросы 178–1283 | Postgres 800% | таблицы `sp_*`, composite отклоняются |
| HAPI | 3009 | 2229 | 1013 | 69–682 | приложение 790–800% | `HFJ_SPIDX_*`, `no-cache` обходит кэш поиска |
| Microsoft | 710 | 623 | 159 | до 2975 | — | SQL Server, индексирует все SP |
| IRIS | 504 | 1090 | 507 | до 16 469 | — | не более 16 одновременных запросов |
| OctoFHIR (09-10, ветка `octofhir`) | 11 812 | 7016 | 5803 | 64,5–98,7 | Postgres 800%; приложение ~2 ядра | `INDEXED_PARAMS` и `COMPOSITE_INDEXES` под тест, анонимный доступ |

Источники строк конкурентов: [SNAPSHOT 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SNAPSHOT_2026-10-30T00:03:50Z.json), [SOURCE_DATA 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SOURCE_DATA_2026-10-30T00:03:50Z.json), [Grafana 10-30](https://grafana.cdr.aidbox.dev/dashboard/snapshot/kX4FlUTLki0a7iSoog4zzvFC20WQRqnV). Строка OctoFHIR взята из другого прогона, поэтому прямое сравнение условно. В прогоне 09-10 Aidbox показал по Grafana 3734–3774 req/s в поиске, то есть 5803 у OctoFHIR выше и в одном прогоне с Aidbox (И).

### Aidbox: дешёвая запись и индексы под тест

Aidbox хранит ресурс одной JSONB-строкой в таблице типа и переносит старую версию в историю только при обновлении и удалении ([Aidbox database schema](https://www.health-samurai.io/docs/aidbox/database/database-schema)). OctoFHIR делает так же: история пишется только AFTER UPDATE/DELETE-триггерами. По умолчанию у Aidbox нет поисковых индексов ([performance at scale](https://www.health-samurai.io/articles/performance-at-scale-baseline.md)), но на стенде миграция `perf-indexes-3` создаёт GIN, GiST, trigram, btree и частичные индексы `WHERE resource @> '{"code":{"coding":[{"code":"8867-4"}]}}'` под конкретные значения теста ([initbundle.json](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/infra/aidbox/initbundle.json)).

Aidbox запускается с `BOX_FHIR_SCHEMA_VALIDATION=true` и `BOX_FHIR_SEARCH_DEFAULT_PARAMS_TOTAL=none`, отклоняет ссылки на несуществующие ресурсы и не выигрывает за счёт пропуска валидации ([docker-compose.yaml](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/docker-compose.yaml), [Aidbox reference validation](https://www.health-samurai.io/docs/aidbox/modules/profiling-and-validation/skip-validation-of-references-in-resource-using-request-header)). Вендор гоняет приватные сборки против стенда в ветках `fast-transaction`, `json-optimize`, `fast-fhirpath`, `pg18-async` ([ветки](https://github.com/HealthSamurai/fhir-server-performance-benchmark/branches/all)). Import Aidbox вырос с ~2,7k до 4262 res/s 2026-07-25 без изменений стенда, то есть за счёт движка (И); числа Aidbox будут двигаться дальше.

Среднее время поиска Aidbox ≈ 30 / 3392 = 8,8 мс против ≤5,2 мс у OctoFHIR (В). Aidbox выигрывает только хвостом: его JVM-слой ограничивает число одновременно активных бэкендов. CRUD p99 Aidbox — 99–140 мс по операциям против 76–90 мс у OctoFHIR 09-10 (И, разные прогоны).

### Medplum: типизированные колонки и ответы 400

Medplum включил только 16 SearchParameter через `enabledSearchParameters` в PR своего CTO ([PR #35](https://github.com/HealthSamurai/fhir-server-performance-benchmark/pull/35), [config.json](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/infra/medplum/config.json)). Неизвестный параметр даёт `badRequest("Unknown search parameter")` до генерации SQL ([medplum search.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/search.ts)). Не включены `component-value-quantity`, `combo-value-quantity`, три composite, subject/encounter/performer у Observation и subject/participant у Encounter — около 10 из 24 групп (В, сопоставление конфига с [searchConfig.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/k6/searchConfig.js)).

Стенд считает все HTTP-ответы, а k6 проверяет только `status === 200` без проверки содержимого ([generate-snapshot.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/scripts/generate-snapshot.js)). Это согласуется с низким CPU Postgres (~120%) в фазе поиска Medplum, но доля 400 в опубликованных данных не видна (Н). Health Samurai сама исключала composite-результаты Medplum как артефакт ([performance at scale](https://www.health-samurai.io/articles/performance-at-scale-baseline.md)).

Выполняемые запросы у Medplum дешёвые: токены хэшируются в UUIDv5 и ищутся через `&&` по `UUID[]` с GIN, даты — по range-колонкам, имена и адреса — по lookup-таблицам ([searchparameter.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/searchparameter.ts), [token-column.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/token-column.ts)). Medplum получает 8 × 16 = 128 соединений против 32 у остальных. Результаты 2026-10-30 (CRUD 3048, Search 3583) выбиваются из его 60-дневной истории (~1,5–2,1k и ~2,0–2,5k) и требуют повторного прогона (Н).

### Остальные и честность сравнения

WSO2 — новый Go-сервер с одной таблицей `resources`, историей и таблицами `sp_*`; composite-поиск он отклоняет («fail closed»), поэтому его RPS тоже частично состоит из ошибок (В, [DESIGN.md](https://github.com/wso2/fhir-server/blob/main/DESIGN.md)). HAPI пишет строки `HFJ_SPIDX_*` на каждое значение и кэширует поиск 60 с, но `Cache-Control: no-cache` стенда этот кэш обходит ([HAPI JPA configuration](https://hapifhir.io/hapi-fhir/docs/server_jpa/configuration.html)). Microsoft пишет все SP через `MergeResources` с табличными параметрами ([MergeResources.sql](https://github.com/microsoft/fhir-server/blob/main/src/Microsoft.Health.Fhir.SqlServer/Features/Schema/Sql/Sprocs/MergeResources.sql)), IRIS ограничен 16 одновременными запросами.

Честность требует признать и настройку OctoFHIR под тест. `INDEXED_PARAMS` перечисляет ровно параметры поиска стенда, а `COMPOSITE_INDEXES` создаёт частичные индексы под горячие LOINC-коды, с пометкой «benchmark-tuned opt-in» ([docker-compose.yaml:174-181](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L174-L181)). Это та же категория, что частичный индекс Aidbox по 8867-4; отличие в том, что у OctoFHIR каждая из 24 групп представлена единственной серией с меткой `status: '200'` (И, по меткам Grafana).

| Идея конкурента | Применимость к OctoFHIR | Статус |
|---|---|---|
| История только при update/delete (Aidbox) | уже так | сделано |
| `_total=none` по умолчанию (Aidbox, Medplum) | уже так | сделано |
| Типизированные колонки поиска (Medplum) | только как ускоритель с перепроверкой, как колонка `ucum` | рассмотреть точечно |
| Подсказки индексов и статистика использования параметров (Aidbox) | расширить `query_analyzer` (`crates/octofhir-db-postgres/src/query_analyzer.rs:413`) | идея |
| Отключение SP с ответом 400 (Medplum) | отвергнуто | см. ниже |
| Redis-кэш чтений (Medplum) | moka-кэш с инвалидацией уже есть; сервер одноэкземплярный | не нужно |

## Зависимости и PostgreSQL 18

Горячий путь уже на последних версиях — sqlx 0.9.0, tokio 1.54.1, axum 0.8.9, tower 0.5.3, serde_json 1.0.154, tracing 0.1.44, mimalloc 0.1.52 — поэтому обновления дадут гигиену, а не крупный прирост. Версии взяты из `Cargo.lock` и crates.io на 2026-10-30. Цель работы с зависимостями — убрать 92 строки дубликатов в `cargo tree -d` и устранить известные дефекты.

### Порядок обновлений

| Шаг | Что | Версии | Ломающие изменения | Риск | Польза |
|---|---|---|---|---|---|
| 1 | Патч-пакет `cargo update -p …` | hyper 1.10.1→1.12.0, hyper-util 0.1.21, moka 0.12.16, reqwest 0.13.5, uuid 1.29, time 0.3.55, rustls 0.23.46, aws-lc-rs 1.19, arc-swap 1.9.3 | нет (semver-minor) | низкий | исправления hyper в разборе и `max_header_size` ([hyper CHANGELOG](https://github.com/hyperium/hyper/blob/master/CHANGELOG.md)); фикс вытеснения moka касается не используемой у нас `EvictionPolicy::lru` ([moka CHANGELOG](https://github.com/moka-rs/moka/blob/main/CHANGELOG.md)) |
| 2 | Дедупликация своих крейтов | tower-http 0.6.11+0.7.0 → 0.7.1; reqwest 0.12.28 (canonical-manager) → 0.13; octofhir-ucum 0.5.2 (cql) → 0.8 | tower-http 0.7: обработка `*` в compression, `SizeAbove` стал u64, удалены пустые фичи ([tower-http CHANGELOG](https://github.com/tower-rs/tower-http/blob/main/tower-http/CHANGELOG.md)) | низкий–средний; сжатие выключено | время сборки, размер бинаря |
| 3 | JWT: бэкенд `rust_crypto` → `aws_lc_rs` в 10.x, затем 10→11 | jsonwebtoken 10.4.0 → 11.1.0 | 11.0: `Algorithm` стал `non_exhaustive`, `Jwk.thumbprint` возвращает `Result`, удалён `insecure_disable_signature_validation` ([jsonwebtoken CHANGELOG](https://github.com/Keats/jsonwebtoken/blob/master/CHANGELOG.md)) | средний | aws-lc-rs уже в графе через rustls; выигрыш проверки подписи не измерен |
| 4 | rquickjs | 0.11.0 → 0.14.0 | `as_bytes` стал `unsafe fn`, изменён trait `Loader`, удалён `StaticJsFunction` ([rquickjs CHANGELOG](https://github.com/DelSkayn/rquickjs/blob/master/CHANGELOG.md)) | средний | 0.13 исправляет deadlock при удалении `Context` с фичей `parallel`, которую мы используем |
| 5 | OpenTelemetry синхронно | opentelemetry 0.31→0.33, tracing-opentelemetry 0.32→0.34 | `SpanExporter` берёт `&self`, `on_end` принимает `FinishedSpan` ([otel-sdk CHANGELOG](https://github.com/open-telemetry/opentelemetry-rust/blob/main/opentelemetry-sdk/CHANGELOG.md)) | средний | меньше клонов span, исправлен deadlock при входе в span ([tracing-opentelemetry CHANGELOG](https://github.com/tokio-rs/tracing-opentelemetry/blob/v0.1.x/CHANGELOG.md)) |
| — | Ждать | async-graphql 8 (RC), redis 2 (RC), dashmap 7 (RC без движения ~19 мес.) | много | высокий | — |
| — | Только при включённом gzip | flate2 с `zlib-rs` | нет | `unsafe` внутри ([flate2 README](https://github.com/rust-lang/flate2-rs/blob/main/README.md)) | быстрее сжатие |

Тулчейн: edition 2024, локально rustc 1.99.0, нижняя граница MSRV задаётся `sqlx-core 0.9.0` (1.94). Файла `rust-toolchain.toml` в репозитории нет. Для воспроизводимого PGO его стоит добавить: профиль должен собираться той же версией LLVM (В).

### PostgreSQL 18 и окружение

Стенд уже работает на `postgres:18`, а локальная разработка и CI — на `postgres:16-alpine` (`docker-compose.yml:3`, `.github/workflows/benchmark.yml`). Это первое, что нужно выровнять. Ниже — возможности 18-й версии, значимые для OctoFHIR.

| Возможность PG18 | Что даёт OctoFHIR | Действие |
|---|---|---|
| Асинхронный ввод-вывод (`io_method` = `worker` по умолчанию, `io_uring`) | seq и bitmap heap scan на холодном кэше: 15,8 с на PG17 против 5,7 с с `io_uring` ([pganalyze](https://pganalyze.com/blog/postgres-18-async-io)); на тёплом стенде мало | `io_uring` в своих Linux-развёртываниях |
| Skip scan в btree | индекс `(code, value)` обслуживает фильтр только по value ([PG18 release notes](https://www.postgresql.org/docs/18/release-18.html)) | учесть в composite-индексах |
| OR-условия в массивы | токены через запятую и раскрытые ValueSet могут стать одним `= ANY` | проверить `EXPLAIN` на `status=finished,in-progress` |
| `RETURNING OLD/NEW` | может заменить CTE `previous AS MATERIALIZED (… FOR UPDATE)` в update (`crates/octofhir-db-postgres/src/queries/crud.rs:823-848`) | прототип с проверкой семантики блокировок |
| Виртуальные generated-колонки | не индексируются ни в 18, ни в 19 RC ([PG18 generated columns](https://www.postgresql.org/docs/18/ddl-generated-columns.html)) | оставить STORED (`profile`, `ucum`) |
| Исправление ложной ошибки про индексы на виртуальных колонках (март 2026) | касается функциональных индексов OctoFHIR ([pgsql-committers](https://www.postgresql.org/message-id/E1w4zA1-001DwY-2l%40gemulon.postgresql.org)) | держать последний минорный 18.x |
| Контрольные суммы данных по умолчанию в `initdb` | безопасность; цена записи не измерена | оставить включёнными |
| `EXPLAIN ANALYZE` с BUFFERS по умолчанию, байты и строки WAL в `pg_stat_io` | проще диагностика | использовать в методике |
| Параллельная сборка GIN | быстрее бутстрап функциональных и document GIN | ничего не делать |
| `pg_upgrade` сохраняет статистику, кроме расширенной | после апгрейда нужен `REINDEX` триграммных GIN и пересоздание `CREATE STATISTICS` | добавить в runbook |
| `uuidv7()` | не нужен: id генерируются в Rust | — |

PostgreSQL 17 дал до 2× пропускной способности WAL при высокой конкуренции и в 20 раз меньше памяти вакуума ([PG17 announcement](https://www.postgresql.org/about/news/postgresql-17-released-2936/)). PostgreSQL 19 находится в стадии RC1: JIT выключен по умолчанию, TOAST по умолчанию сжимается lz4, появились параллельные воркеры автовакуума и `REPACK (CONCURRENTLY)` ([PG19 release notes](https://www.postgresql.org/docs/19/release-19.html)). Переходить на него до финального релиза не стоит.

## Отвергнутые варианты

Отвергнуто 20 вариантов: 11 угрожают сохранности данных, корректности или авторизации, 5 не доказаны или вне объёма, 4 противоречат FHIR или честному сравнению. Ниже таблица и отдельно — найденные риски, которые стоит исправить независимо от скорости.

| Вариант | Почему отвергнут | Категория |
|---|---|---|
| `synchronous_commit = off`, даже на транзакцию | при сбое теряются последние подтверждённые транзакции, окно до 3 × `wal_writer_delay` ([Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)) | небезопасно |
| `fsync = off` | сбой может привести к произвольной порче БД (там же) | небезопасно |
| `full_page_writes = off` | разорванные страницы после сбоя ([WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)) | небезопасно |
| UNLOGGED-таблицы для живых данных | усекаются после сбоя и не реплицируются ([CREATE TABLE](https://www.postgresql.org/docs/current/sql-createtable.html)) | небезопасно |
| `wal_level = minimal` ради COPY без WAL | делает прежние base backup непригодными для восстановления, «may lead to data loss» ([Populating a Database](https://www.postgresql.org/docs/current/populate.html)) | небезопасно |
| Асинхронная или отложенная индексация поиска | поиск пропускает только что записанное | неверные результаты |
| Кэш результатов поиска (как 60 с у HAPI) | устаревание и зависимость от контекста авторизации | неверные результаты, authz |
| Убрать перепроверку за lossy-индексами (trigram, GiST hull) | ложноположительные совпадения | неверные результаты |
| Партиционировать текущие таблицы по времени | PK должен включать ключ партиции, уникальность `id` не гарантируется | небезопасно |
| Отключить контрольные суммы данных | теряется обнаружение порчи | небезопасно |
| Кэш «ресурс существует» между запросами | без инвалидации на удаление пропускает висячие ссылки | неверные результаты |
| `sonic-rs` или `simd-json` | большая `unsafe`-поверхность, а разбора JSON на горячем пути почти нет ([sonic-rs](https://github.com/cloudwego/sonic-rs)) | не доказано |
| Замена sqlx на tokio-postgres или конвейерный драйвер | быстрый путь CRUD уже один оператор; зависимые круги убираются CTE ([tokio-postgres](https://docs.rs/tokio-postgres/latest/tokio_postgres/), [sqlx#2007](https://github.com/launchbadge/sqlx/issues/2007)) | не доказано |
| `force_generic_plan` | плотность значений FHIR сильно разная, generic-план ошибается | не доказано |
| gzip на стенде | тратит CPU ради полосы, которой хватает в docker-сети | не доказано |
| Hash-партиционирование текущих таблиц | поиск идёт по `resource`, прунинга нет | не доказано |
| Отключать SP и отвечать 400, как Medplum | расходится с CapabilityStatement и накручивает RPS ошибками | против FHIR и честности |
| Таблицы извлечённых SP или колонка на параметр (HAPI, Microsoft, Medplum) | новый SearchParameter требует `$reindex` ([Microsoft Learn](https://learn.microsoft.com/en-ca/azure/healthcare-apis/fhir/how-to-do-custom-search), [Smile CDR](https://smilecdr.com/docs/fhir_repository/search_parameter_reindexing.html)) | против модели OctoFHIR |
| `X-Skip-Validation` или `skipValidation` в прогонах стенда | сохраняет неконформные данные; Aidbox валидирует | против FHIR и честности |
| Менять cpuset и квоты стенда | стенд не наш; можно только предложить апстриму | против честности |

### Риски безопасности и корректности, найденные по ходу

| Находка | Где | Почему важно | Исправление |
|---|---|---|---|
| `X-Skip-Validation` действует для любого автора записи | `crates/octofhir-server/src/handlers.rs:27-39` | любой клиент с правом записи сохраняет неконформные данные | привязать к роли или scope |
| В репозитории `allow_skip_validation = true` | `octofhir.toml:63` | по умолчанию в коде `false` (`crates/octofhir-server/src/config.rs:735`), а образец конфига включает обход | вернуть `false` |
| `$import skipValidation` игнорирует `allow_skip_validation` | `crates/octofhir-server/src/operations/bulk/import.rs:220-225` | обход валидации без общего переключателя | проверять переключатель и роль |
| Нет `ORDER BY` без `_sort` при OFFSET-пагинации | `crates/octofhir-search/src/sql_builder.rs:1249-1251`, `:1272-1285` | дубли и пропуски между страницами | `ORDER BY id` и keyset, см. №14 |
| Возможно, не проверяется `ext-1` | поиск `ext-1` в `../fhirschema/octofhir-fhirschema/src` пуст | возможный пробел конформности (Н) | тест: extension с value и вложенными extension должен падать; нативная проверка |
| `_total=estimate` ведёт себя как `none` | `crates/octofhir-search/src/params_converter.rs:1293-1297` | клиент не получает оценку | оценка из `EXPLAIN (FORMAT JSON)` с параметрами ([Count estimate](https://wiki.postgresql.org/wiki/Count_estimate)) |
| `_offset` не ограничен | `crates/octofhir-search/src/params_converter.rs:1277-1281` | глубокий OFFSET — дешёвый способ нагрузить БД | лимит и ссылка на курсор |
| Гонка проверки ссылки и удаления на быстром пути | `crates/octofhir-server/src/handlers.rs:1015-1017` | осознанный компромисс | документировать; при встраивании в `INSERT` можно добавить `FOR KEY SHARE` |
| Блокирующий `std::sync::Mutex` QuickJS в async и выбор рантайма по кругу | `crates/octofhir-auth/src/policy/quickjs.rs:87-138` | медленный скрипт блокирует воркер tokio | `try_lock` по пулу |

## Методика измерений и CI-гейты

Методика строится на трёх уровнях: инструкционные микробенчмарки на каждый PR, ночной k6 на точной копии стенда HealthSamurai и гейты корректности, без которых ускорение не принимается (FHIR262 не ниже 98,7%). Ни одна цифра этого отчёта не получена локальным запуском; каждый эксперимент выше нужно прогнать по этой схеме.

### Воспроизведение стенда

Основа — compose ветки `octofhir` ([f17bfaf](https://github.com/HealthSamurai/fhir-server-performance-benchmark/tree/f17bfaf)): Postgres 18 с 8 CPU и 30 ГБ, OctoFHIR с 8 CPU и 20 ГБ, k6 без лимитов на том же хосте. Образ OctoFHIR тянется с `pull_policy: always`, поэтому для своей сборки нужен override на локальный образ. Переменные окружения нужно копировать дословно, включая `INDEXED_PARAMS`, `COMPOSITE_INDEXES`, `CHECK_TARGET_PROFILE=true` и `FETCH_EXTERNAL_REFERENCES=true` ([docker-compose.yaml:150-191](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L150-L191)).

Внешний сервер терминологии и разыменование внешних ссылок добавляют сетевой шум; это нужно фиксировать в каждом отчёте. Для проверки CFS-гипотезы нужен Linux-хост с более чем 8 ядрами и k6 на отдельных ядрах или машине. На macOS Docker работает в VM, поэтому лимиты там ограничивают vCPU VM; такие прогоны годятся только для относительного A/B, минимум по 3 чередующихся прогона.

Порядок фаз повторяет Drone: prewarm, CRUD, Import, `vacuumdb --analyze`, Search ([.drone.yml](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/.drone.yml)). Нужен второй вариант без `vacuumdb`, как в `main` с 2026-10-28. Ночной workflow репозитория стоит перевести с `postgres:16-alpine` на `postgres:18` и на скрипты HealthSamurai (`.github/workflows/benchmark.yml`); собственный `k6/benchmarks/crud.js` использует 100 VU, а не 300.

### Что снимать в каждой фазе

| Метрика | Источник | Зачем |
|---|---|---|
| `nr_throttled`, `throttled_usec` | `/sys/fs/cgroup/cpu.stat` контейнеров Postgres и OctoFHIR | подтверждение H1 |
| avg, med, p95, p99, max и счёт по статусам | `k6 run --summary-trend-stats "avg,med,p(95),p(99),max"` | среднее и хвост раздельно; доля не-200 |
| Топ операторов, время планирования | `pg_stat_statements` со сбросом перед фазой и `track_planning = on` ([docs](https://www.postgresql.org/docs/current/pgstatstatements.html)) | CPU Postgres на форму запроса |
| События ожидания | сэмплы `pg_stat_activity` каждые 100 мс или `pg_wait_sampling` после `pg_stat_statements` в preload ([pg_wait_sampling](https://github.com/postgrespro/pg_wait_sampling/blob/master/README.md)) | `Lock/advisory`, `LWLock/WALWrite`, `Client/ClientRead` |
| WAL и контрольные точки | `pg_stat_wal`, `pg_stat_checkpointer`, `pg_stat_io` | байты WAL и FPI на ресурс |
| Медленные планы | `auto_explain` с `log_analyze`, `log_buffers` ([docs](https://www.postgresql.org/docs/current/auto-explain.html)) | регрессии планов под нагрузкой |
| CPU приложения | samply или `just flame` на профиле `profiling`; tokio-console в сборке с `tokio_unstable` ([console-subscriber](https://github.com/tokio-rs/console/blob/main/console-subscriber/README.md)); pprof-rs за admin-эндпоинтом ([pprof-rs](https://github.com/tikv/pprof-rs/blob/master/README.md)) | горячие функции, блокировки воркеров |

### Гейты

На каждый PR — инструкционные бенчмарки: gungraun (бывший iai-callgrind) с мягкими лимитами 2–5% ([gungraun](https://github.com/gungraun/gungraun)) или CodSpeed в режиме simulation, как в ruff ([ruff ci.yaml](https://github.com/astral-sh/ruff/blob/main/.github/workflows/ci.yaml)). Кандидаты: компиляция поиска (`crates/octofhir-search/benches/date_search.rs`), `validation_bench`, `quickjs_bench`, сериализация Bundle. Счёт инструкций детерминирован и не шумит на общих раннерах.

Ночью — k6 со сценариями HealthSamurai на фиксированном self-hosted раннере, базовый и кандидатный SHA подряд на одном хосте, как в регрессионном наборе Vector ([Vector regression.yml](https://github.com/vectordotdev/vector/blob/master/.github/workflows/regression.yml)). Пороги — t-test или IQR в Bencher ([Bencher thresholds](https://bencher.dev/docs/explanation/thresholds/)) и `thresholds` k6 с `abortOnFail` ([k6 thresholds](https://grafana.com/docs/k6/latest/using-k6/thresholds/)). Отслеживаются RPS, p99 по каждой группе и операции и CPU Postgres на запрос.

Гейты корректности обязательны, потому что стенд проверяет только `status === 200`:

| Гейт | Как | Порог |
|---|---|---|
| Конформность | FHIR262 | не ниже 98,7% |
| Равенство результатов поиска | множества `id` по 24 группам стенда против базового SHA | побитовое совпадение |
| Условные записи | 50 параллельных одинаковых If-None-Exist, в том числе с цепочкой | ровно 1 ресурс |
| Пагинация | листание при параллельных вставках | нет дублей и пропусков |
| Сохранность | `kill -9` Postgres во время CRUD, затем сверка всех подтверждённых 2xx `id` | 100% на месте |
| Авторизация | запрещённый пользователь с совпадающим `If-None-Match` | 403, не 304 |
| Отзыв токена | запрос сразу после отзыва | 401 |

### Предложения апстриму стенда

Публиковать итоговый p99 за прогон или `max_over_time` вместо среза с шагом 5 минут: срез исказил p99 Aidbox 09-10 в холодной первой минуте. Считать в RPS только ответы 200; прецедент — PR #31, который перестал засчитывать ошибки Microsoft как пропускную способность ([PR #31](https://github.com/HealthSamurai/fhir-server-performance-benchmark/pull/31)). Добавить на дашборд счётчики CFS-троттлинга контейнеров.

## Источники

Ниже 89 уникальных внешних источников, на которые есть ссылки в тексте; пути вида `file:line` относятся к репозиторию server-rs на коммите 536cfc2, пути с `../` — к соседним репозиториям fhirschema и fhirpath-rs.

- [SOURCE_DATA OctoFHIR 2026-09-10](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_2026-09-10T04:24:44Z.json)
- [SOURCE_DATA OctoFHIR 2026-07-25](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/octofhir/SOURCE_DATA_2026-07-25T15:12:45Z.json)
- [SNAPSHOT 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SNAPSHOT_2026-10-30T00:03:50Z.json)
- [SOURCE_DATA 2026-10-30](https://storage.googleapis.com/samurai-public/fhir-server-performance-benchmark/SOURCE_DATA_2026-10-30T00:03:50Z.json)
- [Снимок Grafana 2026-09-10](https://grafana.cdr.aidbox.dev/api/snapshots/NjPOE0WYhukEdhuR6wgYYPQpFmj3TMx3)
- [Снимок Grafana 2026-07-25](https://grafana.cdr.aidbox.dev/api/snapshots/jFvY2yVtRyi84FTblc6QLUNpjWGlKVoZ)
- [Снимок Grafana 2026-10-30](https://grafana.cdr.aidbox.dev/dashboard/snapshot/kX4FlUTLki0a7iSoog4zzvFC20WQRqnV)
- [Стенд, ветка octofhir: k6/search.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/k6/search.js#L18-L32)
- [Стенд, ветка octofhir: scripts/generate-snapshot.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/scripts/generate-snapshot.js#L198-L202)
- [Стенд, ветка octofhir: docker-compose.yaml](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L23) (а также строки [144](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L144), [164](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L164), [174-181](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L174-L181), [185](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L185), [150-191](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/docker-compose.yaml#L150-L191))
- [Стенд, ветка octofhir: infra/postgres/postgres.conf](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/infra/postgres/postgres.conf) (строки [5-10](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/infra/postgres/postgres.conf#L5-L10))
- [Стенд, ветка octofhir: .drone.yml](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/.drone.yml) (строка [179](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/f17bfaf/.drone.yml#L179))
- [Стенд, ветка octofhir: дерево f17bfaf](https://github.com/HealthSamurai/fhir-server-performance-benchmark/tree/f17bfaf)
- [Стенд, main: docker-compose.yaml](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/docker-compose.yaml)
- [Стенд, main: infra/aidbox/initbundle.json](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/infra/aidbox/initbundle.json)
- [Стенд, main: infra/medplum/config.json](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/infra/medplum/config.json)
- [Стенд, main: k6/searchConfig.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/k6/searchConfig.js)
- [Стенд, main: scripts/generate-snapshot.js](https://github.com/HealthSamurai/fhir-server-performance-benchmark/blob/cb5c199/scripts/generate-snapshot.js)
- [Стенд: коммит e6a4941 «Remove vacuumns»](https://github.com/HealthSamurai/fhir-server-performance-benchmark/commit/e6a4941)
- [Стенд: коммит 8b71f14 (конфиг Medplum)](https://github.com/HealthSamurai/fhir-server-performance-benchmark/commit/8b71f14)
- [Стенд: PR #35](https://github.com/HealthSamurai/fhir-server-performance-benchmark/pull/35)
- [Стенд: PR #31](https://github.com/HealthSamurai/fhir-server-performance-benchmark/pull/31)
- [Стенд: список веток](https://github.com/HealthSamurai/fhir-server-performance-benchmark/branches/all)
- [xk6-output-prometheus-remote: remotewrite.go](https://raw.githubusercontent.com/grafana/xk6-output-prometheus-remote/main/pkg/remotewrite/remotewrite.go)
- [Linux kernel: CFS Bandwidth Control](https://docs.kernel.org/scheduler/sched-bwc.html)
- [Dan Luu: The container throttling problem](https://danluu.com/cgroup-throttling/)
- [Rust std: available_parallelism](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)
- [HikariCP: About Pool Sizing](https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing)
- [PostgreSQL wiki: Number Of Database Connections](https://wiki.postgresql.org/wiki/Number_Of_Database_Connections)
- [PostgreSQL wiki: Count estimate](https://wiki.postgresql.org/wiki/Count_estimate)
- [PostgreSQL 19 release notes (RC1)](https://www.postgresql.org/docs/19/release-19.html)
- [PostgreSQL 18 release notes](https://www.postgresql.org/docs/18/release-18.html)
- [PostgreSQL 17 announcement](https://www.postgresql.org/about/news/postgresql-17-released-2936/)
- [PostgreSQL: PREPARE](https://www.postgresql.org/docs/18/sql-prepare.html)
- [PostgreSQL: CREATE STATISTICS](https://www.postgresql.org/docs/18/sql-createstatistics.html)
- [PostgreSQL: Generated columns](https://www.postgresql.org/docs/18/ddl-generated-columns.html)
- [PostgreSQL: HOT](https://www.postgresql.org/docs/current/storage-hot.html)
- [PostgreSQL: WAL configuration](https://www.postgresql.org/docs/current/wal-configuration.html)
- [PostgreSQL: Asynchronous commit](https://www.postgresql.org/docs/current/wal-async-commit.html)
- [PostgreSQL: Populating a Database](https://www.postgresql.org/docs/current/populate.html)
- [PostgreSQL: CREATE TABLE](https://www.postgresql.org/docs/current/sql-createtable.html)
- [PostgreSQL: pg_stat_statements](https://www.postgresql.org/docs/current/pgstatstatements.html)
- [PostgreSQL: auto_explain](https://www.postgresql.org/docs/current/auto-explain.html)
- [pgsql-committers: исправление для виртуальных generated-колонок](https://www.postgresql.org/message-id/E1w4zA1-001DwY-2l%40gemulon.postgresql.org)
- [pganalyze: PostgreSQL 18 async I/O](https://pganalyze.com/blog/postgres-18-async-io)
- [pganalyze: ORDER BY + LIMIT](https://pganalyze.com/blog/5mins-postgres-planner-order-by-limit)
- [use-the-index-luke: no-offset](https://use-the-index-luke.com/no-offset)
- [Cybertec: Rules or triggers to log bulk updates](https://www.cybertec-postgresql.com/en/rules-or-triggers-to-log-bulk-updates/)
- [Tiger Data: Speed up triggers by 7× with transition tables](https://www.tigerdata.com/blog/speed-up-triggers-by-7x-with-transition-tables)
- [Tiger Data: Benchmarking PostgreSQL batch ingest](https://www.tigerdata.com/blog/benchmarking-postgresql-batch-ingest)
- [EDB: WAL compression algorithms](https://enterprisedb.com/blog/you-can-now-pick-your-favorite-compression-algorithm-your-wals)
- [Rust Performance Book: build configuration](https://nnethercote.github.io/perf-book/build-configuration.html)
- [cargo-pgo](https://github.com/Kobzol/cargo-pgo)
- [Kobzol: Make the Rust compiler 5% faster](https://kobzol.github.io/rust/rustc/2023/10/21/make-rust-compiler-5percent-faster.html)
- [sqlx: PgConnectOptions](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html)
- [sqlx issue #2007](https://github.com/launchbadge/sqlx/issues/2007)
- [tokio-postgres](https://docs.rs/tokio-postgres/latest/tokio_postgres/)
- [axum: json.rs](https://raw.githubusercontent.com/tokio-rs/axum/main/axum/src/json.rs)
- [axum: ListenerExt](https://docs.rs/axum/latest/axum/serve/trait.ListenerExt.html)
- [sonic-rs](https://github.com/cloudwego/sonic-rs)
- [HAPI JPA configuration](https://hapifhir.io/hapi-fhir/docs/server_jpa/configuration.html)
- [Medplum Project.d.ts](https://app.unpkg.com/@medplum/fhirtypes@4.3.10/files/dist/Project.d.ts)
- [Medplum: paginated search](https://www.medplum.com/docs/search/paginated-search)
- [Medplum: search.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/search.ts)
- [Medplum: searchparameter.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/searchparameter.ts)
- [Medplum: token-column.ts](https://github.com/medplum/medplum/blob/main/packages/server/src/fhir/token-column.ts)
- [WSO2 fhir-server: DESIGN.md](https://github.com/wso2/fhir-server/blob/main/DESIGN.md)
- [Microsoft fhir-server: MergeResources.sql](https://github.com/microsoft/fhir-server/blob/main/src/Microsoft.Health.Fhir.SqlServer/Features/Schema/Sql/Sprocs/MergeResources.sql)
- [Microsoft Learn: custom search parameters](https://learn.microsoft.com/en-ca/azure/healthcare-apis/fhir/how-to-do-custom-search)
- [Smile CDR: search parameter reindexing](https://smilecdr.com/docs/fhir_repository/search_parameter_reindexing.html)
- [Aidbox: database schema](https://www.health-samurai.io/docs/aidbox/database/database-schema)
- [Aidbox: skip validation of references](https://www.health-samurai.io/docs/aidbox/modules/profiling-and-validation/skip-validation-of-references-in-resource-using-request-header)
- [Health Samurai: performance at scale — baseline](https://www.health-samurai.io/articles/performance-at-scale-baseline.md)
- [hyper CHANGELOG](https://github.com/hyperium/hyper/blob/master/CHANGELOG.md)
- [moka CHANGELOG](https://github.com/moka-rs/moka/blob/main/CHANGELOG.md)
- [tower-http CHANGELOG](https://github.com/tower-rs/tower-http/blob/main/tower-http/CHANGELOG.md)
- [jsonwebtoken CHANGELOG](https://github.com/Keats/jsonwebtoken/blob/master/CHANGELOG.md)
- [rquickjs CHANGELOG](https://github.com/DelSkayn/rquickjs/blob/master/CHANGELOG.md)
- [opentelemetry-sdk CHANGELOG](https://github.com/open-telemetry/opentelemetry-rust/blob/main/opentelemetry-sdk/CHANGELOG.md)
- [tracing-opentelemetry CHANGELOG](https://github.com/tokio-rs/tracing-opentelemetry/blob/v0.1.x/CHANGELOG.md)
- [flate2 README](https://github.com/rust-lang/flate2-rs/blob/main/README.md)
- [pg_wait_sampling](https://github.com/postgrespro/pg_wait_sampling/blob/master/README.md)
- [console-subscriber](https://github.com/tokio-rs/console/blob/main/console-subscriber/README.md)
- [pprof-rs](https://github.com/tikv/pprof-rs/blob/master/README.md)
- [gungraun](https://github.com/gungraun/gungraun)
- [ruff ci.yaml](https://github.com/astral-sh/ruff/blob/main/.github/workflows/ci.yaml)
- [Vector regression.yml](https://github.com/vectordotdev/vector/blob/master/.github/workflows/regression.yml)
- [Bencher thresholds](https://bencher.dev/docs/explanation/thresholds/)
- [k6 thresholds](https://grafana.com/docs/k6/latest/using-k6/thresholds/)
