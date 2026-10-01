# Аудит FHIR Search и SQL — 9 сентября 2026

**Сначала исправить корректность кеша, затем оптимизировать обход ссылок.** На текущем коде воспроизведён ответ с 10 неподходящими Observation и `total=0`: SQL от `value-quantity=ge100` повторно используется для `ge200`. Среди измеренных запросов основные затраты — chained search, `_has` и `_revinclude`. Уже существующие индексы позволяют существенно улучшить отдельные варианты этих запросов; оснований начинать с отдельного поискового хранилища нет.

Исследование не вносит исправлений в продуктовый код. Схемы и индексы создавались только внутри новых одноразовых PostgreSQL-контейнеров с синтетическими данными. Контейнеры завершены. На старте рабочего дерева уже были изменения `Cargo.toml`, `Cargo.lock` и файлы k6; они не являются результатами этого аудита. Сборки выполнялись в текущем рабочем дереве, а не на чистом HEAD.

**Доказательства и границы измерений.**

- [probe.rs](probe.rs) вызывает реальные `parse_query_string`, native IR, SQL renderer, raw search, миграции и создание индексов. Это отдельная диагностическая программа, не тест, подключённый к продуктовой сборке.
- [results.jsonl](results.jsonl) — итоговый запуск; файлы `*.json` рядом содержат SQL, параметры и полные `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)`.
- PostgreSQL **18.4**, aarch64, Docker/OrbStack; образ `postgres:18`. Compose проекта предусматривает PostgreSQL 16. Это разные окружения: перед внедрением повторить на целевой major-версии.
- 20 002 Patient и 20 001 Observation до семантических проверок; 50% синтетических Patient имеют `gender=female`, 10% — `family=Smith`, identifiers уникальны. Каждая синтетическая Observation ссылается на соответствующего Patient; 10% имеют исследуемый code/quantity. Данные компактные, без больших narrative/attachment. Fixture упрощает клинические ограничения и не проходит полноценную FHIR-валидацию.
- Один клиент, pool=1, JIT выключен, параллельные SQL workers выключены, statement timeout=5 s. Начальные замеры — после загрузки и ANALYZE; отдельные замеры после VACUUM ANALYZE помечены `*_clean_*`.
- SearchParameter registry задан вручную на основе существующего `search_explain_test.rs`, без загрузки полного `hl7.fhir.r4.core`. В частности, его `code-value-quantity` искусственно привязан к `Observation.component`: замер `composite` проверяет коррелированный компонентный SQL, а не точную конфигурацию стандартного top-level параметра. Для релизной проверки использовать определения из пакета и соответствующий компонентный параметр.
- 364 unit-теста `octofhir-search` прошли. Существующий ignored EXPLAIN-тест падает до SQL: `Postgres::default()` в testcontainers-modules 0.15.0 — **11-alpine**, миграция требует `jsonpath`; далее в миграции есть multirange. После смены образа тесту также нужны bootstrap helpers и актуальные ожидания SQL: он ещё ожидает старые reference/quantity/token формы.
- HTTP-сервер с этой сборкой и изолированной конфигурацией не запускался. **HTTP p50/p95/p99, HTTP throughput и поведение полного middleware не измерены.** Storage timing не переименован в HTTP latency. [SQL client percentiles](SQL-SAMPLES.md) приведены отдельно. SQL samples по 20 последовательных выполнений — короткий smoke, не нагрузочный тест и не надёжная оценка хвостов.
- Проверка сборки с `--locked` обнаружила несогласованность текущих dependency inputs и lockfile. Для аудита использованы успешно собранные offline зависимости; `run.py` повторно компилирует probe с ними, не переписывая lockfile. Это ограничение воспроизводимости сборки текущего рабочего дерева.

**Карта архитектуры.**

| Этап | Фактическая реализация | Что выполняется |
|---|---|---|
| HTTP GET/POST | [handlers.rs:2807](../../../crates/octofhir-server/src/handlers.rs#L2807), `search_resource`, `search_resource_post` | Проверка типа ресурса, `Prefer: handling`, raw URL; POST объединяет URL и form body, сохраняя повторения |
| Parse | [params_converter.rs:1031](../../../crates/octofhir-search/src/params_converter.rs#L1031) | URL decode; `SearchParams.parameters: HashMap<String, Vec<String>>`; count/offset/sort/total отдельно |
| Терминология | [queries/search.rs:378](../../../crates/octofhir-db-postgres/src/queries/search.rs#L378), `terminology_preprocess.rs` | Асинхронное раскрытие `:in/:not-in/:above/:below`, лимит по конфигурации, затем обычные predicates |
| Registry и нормализация | `loader.rs`, `registry.rs`, [params_converter.rs:136](../../../crates/octofhir-search/src/params_converter.rs#L136) | Пакеты canonical-manager, element hints; детерминированный порядок ключей, модификаторы, prefixes, unknown warnings |
| IR и SQL | `types/mod.rs`, `ir/ast.rs`, `ir/render.rs`, `ir/chain.rs`, `sql_builder.rs` | Typed clauses → SQL conditions; верхний query builder получает также Raw SQL conditions; это не полноценный cost-based optimizer всего дерева |
| БД | [queries/search.rs:884](../../../crates/octofhir-db-postgres/src/queries/search.rs#L884) | SQLx PostgreSQL, `resource::text`, fetch_all текущей страницы; отдельный COUNT по запросу |
| Связанные ресурсы | `resolve_includes_revincludes_raw`, строки 1366–1700 того же файла | После основной страницы: include/revinclude, параллельно между спецификациями; по target types внутри include — последовательные запросы |
| Bundle | [octofhir-api/src/lib.rs:1040](../../../crates/octofhir-api/src/lib.rs#L1040) | RawJson, fullUrl, mode match/include/outcome, pagination links; глобальной дедупликации entries здесь нет |
| Projection | [handlers.rs:3418](../../../crates/octofhir-server/src/handlers.rs#L3418) | `_summary`/`_elements` превращают готовый Bundle в Value и затем удаляют поля/entries |

Основной HTTP type-search напрямую вызывает postgres raw executor через `read_db_pool`, обходя универсальный `FhirStorage::search`. Pool может быть read replica. Storage/transaction/batch/system/compartment пути отличаются; исправление только одного endpoint не гарантирует одинаковое поведение остальных.

Фактический backend — `octofhir-db-postgres`; данные лежат в отдельных таблицах `patient`, `observation` и т. п.: `id`, `txid`, timestamps, `resource JSONB`, `status`, отдельная история. Указание на старый `octofhir-db`/in-memory в AGENTS.md не описывает текущий основной путь. `octofhir.toml:56–98` задаёт **R4 + hl7.fhir.r4.core#4.0.1**. В загрузчике есть варианты R4B/R5/R6, но это не доказательство полной совместимости search с каждой версией.

Фильтрация и AND/OR происходят в SQL. Сортировка и OFFSET/LIMIT — в SQL для type-search. Rust обрезает lookahead, собирает Bundle и projection. System search запрашивает типы последовательно и повторно пагинирует в Rust. Отдельного N+1 чтения каждого основного ресурса в raw-пути нет.

**Приоритеты: подтверждение отделено от ожидаемого эффекта.**

| Приоритет | Проблема и воспроизводящий запрос | Доказательство | Направление |
|---|---|---|---|
| P0, корректность | Кеш SQL: сначала `Observation?value-quantity=ge100`, затем `ge200` | Второй запрос: 10 entries, `has_more=true`, `total=0`; без кеша — 0 entries | Запретить повторное использование несовместимого SQL; затем полноценно параметризовать literals |
| P0, корректность | `Patient?birthdate=1985-03-12` | Возвращает fixture с `birthDate:"1985"` | Single-value fast path обязан проверять containment для eq |
| P0, корректность | `Observation?date=gt2025&date=lt2026` на Timing с 2020 и 2030 | Пусто, тогда как независимые occurrences должны находить оба условия на разных значениях | Не объединять независимые existential predicates в один интервал |
| P0, надёжность | `Observation?subject:Patient.family=李` | Пойман panic: byte index 2 не является UTF-8 boundary | Общий UTF-8-safe parser для прямого и chained пути |
| P1, измеренная стоимость | `Observation?subject:Patient.family=Smith` | 291.729 ms; ~198 тыс. Function Scan loops | Извлечение ключей ссылки + lookup по PK; точная поддержка ссылок обязательна |
| P1, измеренная стоимость | `Patient?_has:Observation:subject:code=…` | 235.420 ms; ~200 тыс. Function Scan loops | Indexable join/semijoin по references, стратегия от селективной стороны |
| P1, измеренная стоимость | `Patient?_id=p-10&_revinclude=Observation:subject` | Полный scan Observation; storage 221.664 ms; SQL probe 416.400 ms | Использовать существующий reference GIN там, где сохранена семантика |
| P1, корректность/ресурсы | `_summary=count`, `_count=0` | 10/1 загруженных entries; `total=None` на storage boundary | Отдельный count-only execution path без resource/includes |
| P1, корректность | System search `_type=Patient,Observation&_offset=10` | Offset применяется внутри каждого SQL и снова в Rust | Одна глобальная пагинация; общий порядок/total/includes |
| P1, корректность | `_sort=family` | SQL `resource->'name'->>'family'`; для массива name даёт NULL | Typed array-aware sort keys, явный стабильный tie-breaker |
| P1, корректность | Две одинаковые `_include=Observation:subject` | Один и тот же Patient дважды в included | Дедуп по `(resourceType,id)` с приоритетом match |
| P1, корректность | `family:contains=Smith%20Jones`, значения в двух name | Совпадение по склеенному тексту | Индексный prefilter + точная проверка каждого исходного значения |
| P2, стоимость зависит от нагрузки | `_sort=-_lastUpdated`, глубокий OFFSET | 10.801/11.607 ms, полная выборка 10 001 matching rows для сортировки | Типизированная стабильная пагинация, опциональные индексы по workload |
| P2, локальная лишняя работа | `_count=10` | SQL LIMIT 12, возвращается 10 | Единственный владелец lookahead; LIMIT 11 |
| P2, сопровождение | EXPLAIN smoke использует PostgreSQL 11 и устаревшие SQL expectations | Ошибка миграции, сравнение с текущим renderer | Зафиксировать поддерживаемый PG, bootstrap helpers, исправить fixture |

P0 здесь означает необходимость устранить дефект до доверия результатам оптимизации. Рейтинг performance-работ относится к этому синтетическому workload: частота реальных запросов и production p95 неизвестны.

**Семантика поддерживаемых типов и параметры результата.**

| Тип | Реализовано в основном пути | Ограничения и проверки |
|---|---|---|
| token | `system|code`, `code`, `system|`, `|code`; Identifier, scalar code, Coding/CodeableConcept и массивы; `:not`, `:missing`, Identifier `:of-type` | Generic document `@>` и subtree `@>` — разные индексные выражения. SQL tests различают отсутствие system и пустой system. `token :text` явно запрещён production dispatcher, даже если низкоуровневый renderer существует |
| string | Нормализованный blob для prefix/contains; exact через `= ANY(text[])`; missing по extraction | Blob теряет границы значений. `% smith%` также находит слово внутри `Jon Smith`. Escaped comma разбивается как OR. Rust NFD и SQL unaccent следует сверить на расширенном Unicode-наборе |
| reference | `fhir_extract_ref(...) && ARRAY[...]`; typed/bare ID, несколько targets; `:identifier`, `:missing`; цепочки | Прямой поиск, цепочки и include используют разные нормализации. Прямой version suffix снимается; chain сравнивает equality/suffix LIKE; include regex заканчивается Type/id. Полная эквивалентность relative/absolute/versioned не доказана |
| date | Precision-aware Rust ranges, prefixes, hull GiST, multirange recheck, `_lastUpdated` на колонке | eq scalar guard и fold gt/lt ошибочны в воспроизведениях. ge/le не прошли через тот же fold и сохранили Timing match. Open Period в проверенном запросе найден; это не покрывает все префиксы |
| number | Числовой cast, decimal implicit interval для eq/ne, сравнения и ap | Plain JSON path не является универсальным обходом повторяющихся родителей; числовая сортировка сейчас текстовая. Нужны negative/zero/exponent/precision и array fixtures |
| quantity | Union путей, numeric min/max или range hull, exact JSONPath recheck с system/code в одном элементе | Values встроены в SQL, ломая кеш. В коде сравниваются code **или** unit; полноценных UCUM conversions в поисковом пути не обнаружено. SampledData ветви явно отбрасываются |
| uri | Scalar/array equality, above/below, missing; `_profile` через отдельный profile text[] | Генерируемая profile column опциональна в schema manager: согласованность с конфигурацией search требует проверки. Above/below/array формы не автоматически обслуживаются тем же индексом |
| composite | Компоненты разрешаются через registry по definition URL; для повторяющегося родителя общий `component_elem` | Сохранять корреляцию; независимые EXISTS для code и value недопустимы. Текущий smoke registry не равен полному стандартному registry |

Обычные повторения `a=x&a=y` создают AND, comma-list — OR; это подтверждено converter и unit-тестами. Контрпример выше показывает, почему дальнейшие rewrites должны учитывать кванторы над массивами. Парсинг `\,`, `\|`, `\$`, `\\` не реализован единым escape-aware scanner: converter использует `split(',')`, token/composite — свои split. Сохранённый probe `escaped_comma` показывает два SQL terms вместо literal comma.

`:missing` имеет отдельные branches, но многие читают только первое значение и интерпретируют всё, кроме true, как false. Нужны отдельные правила валидации и проверки пустых массивов, вложенных повторений и extension-only values. Не стоит объявлять весь `:missing` корректным только по наличию renderer.

`_id` и `_lastUpdated` используют row columns. `_count`, `_offset`, `_sort`, `_total`, include/revinclude поддержаны; `_estimate` алгоритма подсчёта нет: `_total=estimate` идёт без COUNT, затем handler может вывести total по последней странице. При пустой странице за концом `resolved_search_total(None,false,1000,0)` возвращает 1000 — это не точное число результатов.

У `_summary=count` нет отдельного COUNT: handler удаляет entries после выполнения поиска, а total часто отсутствует. `_count=0` clamped до 1. Нормативный count-only результат должен включать total, без entries и pagination links. [FHIR R4 count/summary](https://hl7.org/fhir/R4/search.html#count).

В type-search нет default ORDER BY; `_sort` не добавляет id tie-breaker. Это снижает стоимость несортированного LIMIT, но стабильность offset pages не обеспечена. Полный SELECT включает resource::text; SQL не всегда обязан материализовать JSON до LIMIT, однако измеренная сортировка работает над широкими проекциями и всеми подходящими строками. Перенос отбора IDs до загрузки JSON имеет смысл для сортировок/глубоких страниц и больших ресурсов; для простого streaming LIMIT доказанного выигрыша нет.

Специальные возможности не следует считать работающими по наличию отдельной функции: `_filter` имеет parser/builder, но не зарегистрирован common registry; `_text`/`_content` в common registry не имеют expression, а dispatcher требует expression перед special dispatch; `_query` явно NotImplemented. `_contained`/`_containedType` отнесены к control params и не меняют основной SQL. Wildcard include распознаётся отдельным модулем, однако raw executor требует registry lookup и конкретные target types. Эти возможности нуждаются в endpoint-проверке; аудит их не расширяет.

**SQL, планы и варианты изменений.**

Обозначения в сокращённых SQL: `ref_expr(resource)` — **точное** выражение `fhir_extract_ref(resource, ARRAY['$."subject"."reference"[*]'::jsonpath])`; `hull`/`mr` — выражения, сохранённые целиком в соответствующих plan JSON. Сокращения не являются существующими DB-функциями.

| Запрос | PostgreSQL Execution Time, ms | Ключевой план / объём работы |
|---|---:|---|
| identifier редкий, сразу после загрузки | 4.372 | Seq Scan, фильтрует 20 001 строк |
| identifier отсутствует | 4.467 | Seq Scan, полный обход |
| family prefix / exact | 0.831 / 0.469 | Seq Scan с ранней остановкой; частые значения, LIMIT 12 |
| birthdate window | 0.124 | GiST Index Scan |
| family + gender + date | 5.985 | BitmapAnd; формирование bitmap до LIMIT |
| gender без total | 0.029 | Ранняя остановка Seq Scan |
| gender COUNT | 6.183 | Считает всю подходящую выборку |
| reference точечный | 0.048 | Bitmap Index Scan по `idx_observation_subject_ref` |
| chain / `_has` | 291.729 / 235.420 | Nested Loop, Materialize, около 200 тыс. проверок reference |
| сортировка updated_at / OFFSET 9000 | 10.801 / 11.607 | 10 001 matching row; top-N heapsort / quicksort |
| quantity / composite smoke | 2.304 / 0.101 | Quantity bitmap на 20 001 candidate; composite ранний LIMIT |
| `_sort=family` | 17.261 | Сортирует 20 002 строки по NULL-ключу — неверная семантика |

В изученных планах нет temp spill: сортировки в памяти, deep offset занимает около 2.9 MB. Нельзя объявлять работу на диске или нехватку work_mem причиной замедления. Все опубликованные основные `Shared Read Blocks=0`: это в основном warm-buffer measurements, не cold disk benchmark. Числа разных запусков EXPLAIN/storage могут заметно отличаться из-за instrumentation, кешей и фоновой работы; не вычитать их друг из друга как чистую стоимость Rust.

**Кеш SQL: текущая причина и безопасный первый шаг.**

[ir/render.rs:407](../../../crates/octofhir-search/src/ir/render.rs#L407) встраивает quantity value и JSONPath в SQL:

```sql
-- value-quantity=ge100
WHERE fhir_qty_extract_max_numeric(resource, <constant paths>) >= 100
-- value-quantity=ge200 должен дать >= 200
```

В обоих случаях `BuiltQuery.params=[]`. [queries/search.rs:478](../../../crates/octofhir-db-postgres/src/queries/search.rs#L478) строит одинаковый ключ: имя, общий token shape, количество occurrences, prefix ge, pagination. [query_cache.rs:320](../../../crates/octofhir-search/src/query_cache.rs#L320) проверяет лишь количество binds. Поэтому второй запрос получает старый literal `100`. COUNT строится заново и противоречит entries.

Ближайшее исправление — выполнять свежепостроенный main SQL, не использовать небезопасный template cache в raw path. Дальнейшее решение: типизированные bind slots, structural identity **после** терминологии/нормализации/registry resolution, с ordered OR arms и immutable literals в идентичности. Просто включить количество OR values в ключ недостаточно.

```sql
-- Возможная последующая форма для простого numeric prefilter:
WHERE fhir_qty_extract_max_numeric(resource, <constant paths>) >= $1::numeric
```

Для exact JSONPath recheck сохранить indexable оператор `@?` и проверить bind-вариант `$2::jsonpath` на custom/generic plans. Замена на произвольный `jsonb_path_exists(..., vars)` не доказывает сохранения индексного доступа. Риск SQL-cache распространяется и на формы с одинаковым числом binds, но другими OR prefixes/shape; их включить в регрессионную матрицу.

Ни schema change, ни новая FHIR-функциональность не нужны. Ожидаемый результат первого исправления — корректная выдача; ускорение не заявляется. Converter уже выполняется до cache lookup, поэтому кеш экономит лишь часть render/склейки, а не весь parse/IR. Текущие единичные build measurements в debug profile: примерно 36–390 µs после первого запроса; это не release CPU benchmark.

**Chain и `_has`: убрать произведение кандидатов.**

Текущий шаблон:

```sql
SELECT r.resource::text, r.id, ... FROM observation r
WHERE EXISTS (
  SELECT 1 FROM patient t
  WHERE t.status != 'deleted'
    AND EXISTS (
      SELECT 1 FROM jsonb_array_elements(<references of r>) ref
      WHERE ref->>'reference' = 'Patient/' || t.id
         OR ref->>'reference' LIKE '%/Patient/' || t.id
    )
    AND fhir_text_blob(fhir_s_patient_family(t.resource)) LIKE $1
) AND r.status != 'deleted' LIMIT 12;
```

`chain.json`: 111 внешних Observation, 2 001 Patient candidates; Materialize повторяется 111 раз, inner Function Scan 198 177 раз. `_has` аналогично делает 200 178 проверок. Это доказанная причина на fixture, а не предположение о «медленном JSONB вообще».

Измеренный **только для local scalar reference** прототип заменяет условие связи на:

```sql
WHERE t.id = split_part(r.resource->'subject'->>'reference', '/', 2)
  AND split_part(r.resource->'subject'->>'reference', '/', 1) = 'Patient'
```

Получилось **1.404 ms**, 111 PK lookups, против 291.729 ms. План: [chain_local_scalar_candidate.json](chain_local_scalar_candidate.json). Никакого нового индекса не понадобилось.

Это ещё не универсальная замена: поддержать arrays через один lateral extraction, Type/id и принятые сервером абсолютные формы; исключить удалённые ресурсы; сохранить семантику type modifiers, версий, удалённых targets и shared references. Для высокоселективного target фильтра также измерить направление `filtered targets → source reference GIN`, избегая JOIN duplicates через semijoin или dedup только IDs. `_has` требует отдельного SQL-варианта: его ускорение в прототипе не измерено.

Файлы будущего изменения: `ir/chain.rs`, общие reference helpers в `sql_builder.rs`, интеграционные fixtures. Сложность средняя/высокая из-за семантики, а не числа строк SQL. Проверка — равенство множеств IDs на обоих направлениях и числе ссылок 0/1/10/100. Откат — вернуть прежнюю стратегию для выбранной shape, не менять данные.

**Include/revinclude: использовать подходящий reference predicate и дедуплицировать IDs.**

Текущий `_revinclude`:

```sql
SELECT DISTINCT s.resource::text, s.id, s.txid, s.created_at, s.updated_at
FROM observation s
WHERE s.status != 'deleted'
  AND EXISTS (
    SELECT 1 FROM jsonb_array_elements(<references of s>) ref
    CROSS JOIN LATERAL (
      SELECT regexp_match(ref->>'reference', '([A-Za-z]+)/([A-Za-z0-9.-]{1,64})$') AS m
    ) x
    WHERE x.m[1]=$1 AND x.m[2]=ANY($2::text[])
  );
```

Коррелированный regexp не использует existing reference GIN. На 20 001 Observation выполняется 20 001 Function Scan. DISTINCT добавляет Unique/Sort над resource::text, хотя внешний FROM содержит только одну строку на PK. С ним — 416.400 ms, без — 417.905 ms: **ускорение от удаления DISTINCT не подтверждено** на наборе с одним include result.

Проверенный local-reference кандидат:

```sql
SELECT s.resource::text, s.id, s.txid, s.created_at, s.updated_at
FROM observation s
WHERE s.status != 'deleted'
  AND fhir_extract_ref(s.resource,
        ARRAY['$."subject"."reference"[*]'::jsonpath]) && ARRAY[$1];
-- $1 = 'Patient/p-10'
```

Получилось **0.032 ms**, existing `idx_observation_subject_ref`, один результат. [revinclude_local_candidate.json](revinclude_local_candidate.json). Совпадение выражения индекса принципиально: даже добавленный `[*]` в другом месте семантически похожего пути в предварительном эксперименте лишал запрос индексного доступа.

Существующая regex-семантика допускает arbitrary-base suffix; canonical reference GIN хранит absolute URL целиком. Поэтому новый predicate нельзя безусловно поставить вместо старого: он потеряет часть absolute matches. Нужны отдельные доказанные fast paths/более полная representation и явная политика bases; versioned references также различаются. Изменение в `queries/search.rs`, helpers, API bundle dedup. Индекс не добавлять, пока можно использовать существующий. Перенос чтения source JSON для `_include` из БД в Rust пока не обоснован: Rust raw path специально избегает JSON parsing.

Для `_include` JOIN действительно может повторить target. Вместо DISTINCT по полному JSON выделять уникальные target IDs до JOIN к ресурсу. После всех спецификаций и итераций дедуплицировать `(type,id)` глобально, сохраняя mode match. Сейчас `try_join_all` умножает work на число specs; максимальная глубина iterate=100 не ограничивает ширину или объём выдачи. Ошибки отдельных include, общий бюджет и cancellation должны быть тестируемыми, а не молчаливым усечением.

**Сортировка, total и projection.**

`build_sort_spec`/`build_order_clause` должны учитывать тип и cardinality: `name[]`, numeric keys и timezone-aware dates нельзя сортировать одинаковым `->>` accessor. Неизвестные поля сейчас могут молча пропускаться. Для корректного sort определить правило выбора из массива и добавить id tie-breaker. Нельзя автоматически вернуть default updated_at sort всем запросам: на frequent predicate он заставляет читать все matches.

Возможный opt-in индекс для частого `_sort=-_lastUpdated`:

```sql
CREATE INDEX CONCURRENTLY ... ON patient
  (updated_at DESC, id DESC) WHERE status != 'deleted';
-- query должен иметь согласованные ORDER BY и predicate.
```

Ожидание — ordered Index Scan с ранним LIMIT при достаточной плотности совпадений. На редких фильтрах он может читать много неподходящих строк; это **не измеренный выигрыш** и не предложение добавить индекс всем таблицам. `schema.rs:275` намеренно не индексирует timestamps. Такой индекс увеличит WAL/размер/стоимость обновлений и ограничит HOT; whole-resource GIN уже ограничивает HOT при изменении resource. Отдельно оценить metadata-only writes. В тестовом отчёте новый timestamp index не создавался.

Keyset возможен поверх стабильного составного ключа:

```sql
WHERE (updated_at,id) < ($cursor_time,$cursor_id)
ORDER BY updated_at DESC,id DESC LIMIT $count_plus_one;
```

Это новый контракт continuation, не drop-in replacement для `_offset` или случайного перехода к странице. Нужны совместимые links, null handling, query identity и политика concurrent updates. Сначала исправить существующую пагинацию, затем вводить опциональный cursor; snapshot consistency не возникает автоматически.

COUNT сейчас последовательный после main SQL; оба могут видеть разные snapshots. Параллельное выполнение может уменьшить latency, но увеличивает одновременную нагрузку и не решает consistency. Window `COUNT(*) OVER()` также не бесплатная оптимизация: заставляет считать все строки до страницы. Сначала count-only path, устранение неверного inferred total и лишнего lookahead, затем измеренная политика `_total`.

`_summary`/`_elements` сейчас разбирают уже полученный Bundle в Value. Для summary=count это полностью устранимая работа; для `_elements` перенос projection в SQL возможен, но нужно сохранить обязательные поля, meta/SUBSETTED и поведение included resources. Выигрыш на больших JSON не измерен.

**Индексы, статистика, prepared plans и стоимость записи.**

В текущем schema builder whole-document GIN `resource jsonb_path_ops`, PK(id), optional profile indexes; functional indexes создаются на bootstrap из `search.indexed_params`. По умолчанию там popular dates, strings, references, но нет `Observation.code`/`value-quantity`. `functional_indexes.rs` умеет subtree token GIN, reference GIN, string trigram GIN, date GiST, quantity min/max btree + hull GiST, targeted partial composite indexes. Это уже существующие возможности конфигурации.

`resource @> ...` может обслуживаться document GIN, `(resource->'code') @> ...` требует соответствующего выражения либо другого SQL. Наличие document GIN не ускоряет автоматически fhir_extract_ref, функции строк, regexp и subtree comparisons. [PostgreSQL JSONB indexing](https://www.postgresql.org/docs/18/datatype-json.html#JSON-INDEXING).

После VACUUM ANALYZE редкий identifier перешёл с Seq Scan 4.372 ms на Bitmap Index Scan, **0.063 ms** с forced custom plan. Это эффект обслуживания/обновления состояния fixture, а не доказательство необходимости нового индекса. Pending-list размеры отдельно не измерены, поэтому нельзя весь эффект приписать только GIN pending list. В сервере уже есть `gin_maintenance.rs` с настраиваемым periodic cleaner (`PostgresConfig::default()` задаёт 120 s); probe через `from_pool` его не запускал. Production cadence/ошибки cleaner и autovacuum надо наблюдать, а не добавлять дублирующую службу.

Для сравнения планов использованы настоящий SQL `PREPARE audit_token(jsonb)` и `EXPLAIN ... EXECUTE`, не только EXPLAIN поверх SQLx wrapper. На rare identifier: custom 0.063, generic 0.040 ms. На common gender: custom Seq Scan **0.022 ms**, generic Bitmap **0.428 ms**; generic оценил 200 rows, bitmap получил 10 001. Это повод измерять распределение параметров и generic/custom counts на реальном pool, **не** глобально выставлять `force_custom_plan`. Механизм выбора планов описан в [PostgreSQL PREPARE](https://www.postgresql.org/docs/18/sql-prepare.html).

На fixture дополнительно включены уже реализованные `Observation.code` и `Observation.value-quantity`: 4 индекса, build 772 ms. Размеры: code GIN 90 112 B; quantity qmin/qmax по 466 944 B, hull 720 896 B. Общий observation document GIN — 6 037 504 B, subject reference GIN — 1 449 984 B. Эти расходы нельзя переносить линейно на production без проверки распределения.

Добавление quantity indexes **не изменило главный page plan**: он остался на document GIN и набрал 20 001 candidate из-за общих unit/system. 2.304 vs 1.522 ms при также изменённом состоянии обслуживания и LIMIT 12/11 — не корректное доказательство ускорения от индекса. Code page тоже и без нового индекса достаточно быстро останавливается на первых frequent matches. Новые индексы заслуживают отдельного теста rare/absent/COUNT; массовое включение не обосновано.

PostgreSQL оценки местами сильно расходятся: quantity document GIN ожидал 2 строки, фактически bitmap получил 20 001; `_has` inner scan ожидал 398, получил 2 001; revinclude ожидал около 10 000 результатов, получил 1. Нужны actual plans на нескольких частотах, ANALYZE после загрузки, statistics targets для релевантных expressions, а не только глобальная настройка planner costs. Удаление «неиспользуемых» индексов по этой короткой сессии недопустимо: production pg_stat_user_indexes/history здесь не исследовались.

**Параметризация и ограничения сложности.**

Основные текстовые/JSON/даты передаются binds через SQLx; schema/resource identifiers в верхнем builder валидируются и quoting централизован. Однако quantity literals/JSONPath — существенное исключение, и `redact_sql_shape` скрывает только номера placeholders, не встроенные значения. Такие debug-логи могут содержать search values.

Reference chain/type/include SQL и functional DDL используют дополнительные format!-пути. Проверить их единым validator/quoting для metadata и explicit targets; наличие `AssertSqlSafe` само по себе ничего не валидирует. В audit инъекции не эксплуатировались и не объявляются доказанными.

Проверенные ограничения: count≤100 по текущей конфигурации, expansion≤500, iterate depth=100. В chain/_has renderer не найден общий depth/term budget; OR/число повторений и ширина include не ограничены единым search budget. `pool.rs` настраивает acquisition timeout, но не PostgreSQL statement_timeout; `read_timeout_ms` конфигурации не является доказательством отмены SQL. Нужны bounded predicates/expansion/include fanout и DB cancellation integration tests; значения лимитов выбирать по измерениям и совместимости API.

**План бенчмарков и регрессий.**

1. Починить measurement fixture: поддерживаемая pinned PostgreSQL major, полный bootstrap helpers, загрузка реальных R4 SearchParameters; убрать SQL expectations старых forms. Проверять не только shape, но и множества IDs.
2. Масштабы 20k → 100k → 1M Patient; Observation 1/5/20 на Patient. Отдельно равномерное и перекошенное распределение, rare/common/empty filters; компактный JSON и 10–100 KB resources; 0/1/10 values и references. Seed только отдельной БД.
3. До измерений сохранить version, resource counts, registry expressions, конфигурацию/набор индексов, relation/index sizes, pool, CPU/RAM, work_mem, jit, planner settings. Отдельные режимы после bulk-load и steady state, warm и cold; cold не симулировать перезапуском чужого сервиса.
4. SQL: полный `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)`, plan/actual rows, loops, heap/index reads, rechecks, sort spills. Для sort сравнить full-resource и IDs-first; для chain оба направления; для GIN exact structural expression. EXPLAIN overhead учитывать отдельно от обычного execution. [PostgreSQL EXPLAIN](https://www.postgresql.org/docs/18/sql-explain.html).
5. HTTP: [http-search.js](http-search.js), local URL, действительный TOKEN из окружения. Сценарии identifier/family/date/combined/reference/chain/has/sort/deep/include/revinclude/total. Сначала 1 VU, затем 8/32/64 с warmup и ≥3 повторами 60–180 s; отдельный arrival-rate тест, чтобы closed-loop не скрывал насыщение. Отчёт: p50/p95/p99 по case, request rate, error rate, timeout/dropped iterations, response bytes и match/include count. DB spans связывать с HTTP по trace id.
6. Запись: при каждом новом индексе измерить create/update/batch p95, WAL bytes, размер индекса, HOT fraction, dead tuples, GIN pending и vacuum time; проводить эти изменения только на клоне fixture.
7. Каждая оптимизация: равенство sets IDs, total, dedup и links до/после. Ошибочные старые результаты фиксируются отдельными correctness tests, а не используются как oracle.

Минимальная регрессионная матрица: cache hit/miss, разные values с одинаковой shape и наоборот, OR prefix в каждой позиции, repeats с перестановкой, terminological expansion меняющегося размера; кириллица/китайский/emoji и escaped separators; decimal precision/negative/ap, units/codes в разных quantity elements; даты year/month/day/timezone, adjacent/disjoint Timing ranges, open Period и mixed gt/lt; composite code/value в одном и разных components; references relative/absolute/remote/versioned, arrays, same ID у разных типов; deleted rows; пустая/последняя/за-пределами страница; equal sort keys; повторные и циклические includes; одновременные updates; все GET/POST/storage/transaction/system/compartment варианты.

**Последовательность небольших PR.**

| PR | Изменение | Эффект / стоимость / откат |
|---|---|---|
| 1 | Устранить небезопасный SQL template reuse; regression `ge100 → ge200` | Исправляет подтверждённую ошибку результата. Небольшой локальный change. Проверить render overhead. Не откатывать включением ошибочного кеша: fallback — свежий builder |
| 2 | UTF-8-safe parser и escape-aware scanner, shared direct/chain | Надёжность и корректные значения; не смешивать с SQL planning. Откат отдельного commit с сохранением regression cases |
| 3 | Date eq guard и запрет некорректного fold independent occurrences | Correctness gate для дальнейшей date оптимизации; без schema changes |
| 4 | Count-only, single lookahead, inferred-total boundaries | Убирает доказанную лишнюю загрузку; корректные count/links; отдельно GET/POST/batch parity |
| 5 | Глобальный include dedup и убрать лишний revinclude DISTINCT | Небольшое изменение; memory-benefit оценить на большом fanout, latency-win пока не доказан |
| 6 | Reference key extraction + chain/revinclude strategies | Наибольший измеренный потенциал; средний объём кода, большая semantic matrix. Переключение по поддержанной shape, сохранение корректного fallback |
| 7 | Typed sorts и system pagination | Более широкий API-risk; один global ordering/offset. Отдельная review/query matrix |
| 8 | Workload-specific индексы и planner/statistics tuning | Только после SQL fixes и A/B. Для live DDL — concurrency/locking plan, rollback отдельных индексов |
| 9 | Optional cursor / IDs-first для тяжёлых страниц | Изменение контракта/плана, вводить отдельно; старые offset clients должны продолжить работать |

Исправление старого EXPLAIN fixture можно подготовить отдельным infrastructure PR параллельно по очереди работ; оно не должно задерживать reproduction cache-regression на PG18. Отдельные sidecar tables, денормализация и result caching пока отложены: сначала использовать существующие PK/functional indexes. Их ввод потребует транзакционной синхронизации create/update/delete, backfill/reindex для новых SearchParameters, контроля write amplification, ключей авторизации и invalidation. Текущие данные этого аудита не доказывают, что эти затраты окупятся.

**Конкретный первый PR: `fix(search): prevent stale SQL reuse in query cache`.**

Изменить [crates/octofhir-db-postgres/src/queries/search.rs](../../../crates/octofhir-db-postgres/src/queries/search.rs) в `execute_search_raw_with_config_inner`: перестать подменять свежий SQL шаблоном, совместимость которого проверяется только по heuristics и числу binds. Сохранить публичные аргументы/API, warnings, terminology preprocessing, count/includes; выполнять результат текущего builder. Не пытаться в этом PR одновременно менять индексы и все quantity renderers.

Добавить `crates/octofhir-db-postgres/tests/search_cache_regression.rs`: отдельный pinned PostgreSQL 18 fixture, migrations + schema + `create_default_search_indexes` helpers, resources с valueQuantity 80/120. Последовательно выполнить ge100, ge200, ge100 с одним экземпляром QueryCache; сравнить с cache-disabled execution и проверить сами quantities, ID sets, total/has_more. Повторить обратный порядок и разные system/unit. Утверждать `total=0 ⇒ entries=[]` для этого count-accurate сценария, а не просто HTTP 200.

Доказательство готовности: regression воспроизводит старый дефект и проходит после исправления; 364 unit-теста остаются зелёными; selective postgres tests проходят на поддерживаемом PG; после запуска полноценного сервера GET/POST повторяют тот же результат. На release workload проверить CPU/build timing с кешем до/после; до такого замера не обещать снижение latency. Риск — дополнительная финальная склейка SQL; безопасность результата выше спорной экономии render. Следующий performance PR — shared reference strategy, с сохранёнными baseline plan files этого аудита.
