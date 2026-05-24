# OctoFHIR Performance Improvement Plan

План сфокусирован на кодовой базе и PostgreSQL-схеме. Без требований к большим
инфраструктурным стендам, шардингу и 100GB+ benchmark-ам.

## Принципы

1. Table-per-resource остается базовой моделью хранения.
2. Индексы добавляются от реальных FHIR-запросов, а не заранее на все поля.
3. JSONB `resource` остается source of truth; search projections являются
   ускорителями для конкретных FHIR search semantics.
4. Любой новый индекс должен иметь понятный read win и понятную write cost.
5. Сначала исправляем query shape и pagination, потом добавляем тяжелые индексы.

## P0: База для нормальной производительности

### 1. Keyset pagination вместо глубокого OFFSET

Файлы:

- `crates/octofhir-search/src/sql_builder.rs`
- `crates/octofhir-db-postgres/src/queries/search.rs`
- `crates/octofhir-db-postgres/src/queries/history.rs`
- `crates/octofhir-server/src/operations/bulk/export.rs`

Что сделать:

- Ввести `_page_token` для search/history/export.
- Default order: `updated_at DESC, id ASC`.
- Любой `_sort` дополнять стабильным `id`.
- Для старого `_offset` оставить compatibility path, но пометить как slow/deprecated.

DB:

```sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_<resource>_active_updated_id
ON "<resource>" (updated_at DESC, id)
WHERE status != 'deleted';
```

Проверка:

- локально: 100k-1M `Observation`;
- сравнить page 1, page 100, page 1000;
- метрики: latency, `EXPLAIN (ANALYZE, BUFFERS)`, rows removed, shared reads.

Риск:

- Paging links должны оставаться корректными при concurrent writes.

### 2. Date search должен реально использовать `search_idx_date`

Файлы:

- `crates/octofhir-search/src/types/mod.rs`
- `crates/octofhir-search/src/params_converter.rs`
- `crates/octofhir-db-postgres/src/search_index.rs`

Что сделать:

- Для registered date SearchParameter строить SQL через `search_idx_date`.
- JSONB date casts оставить только как fallback/debug mode.
- Проверить FHIR prefix semantics: `eq`, `ne`, `gt`, `lt`, `ge`, `le`, `sa`, `eb`,
  `ap`.

Проверка:

- unit tests на SQL generation;
- integration test: `Observation?date=ge2024-01-01&date=lt2025-01-01`;
- `EXPLAIN` должен идти от `search_idx_date`, не от JSONB cast по resource table.

Риск:

- Ошибка в range semantics даст неверный FHIR search result.

### 3. Transaction conditional logic только primary-consistent

Файлы:

- `crates/octofhir-server/src/handlers.rs`
- `crates/octofhir-db-postgres/src/storage.rs`
- `crates/octofhir-db-postgres/src/transaction.rs`

Что сделать:

- Conditional create/update/delete внутри transaction не должны зависеть от read
  replica или async search-index lag.
- Pre-scan conditional POST перенести внутрь native transaction или сделать
  explicit primary read path.

Проверка:

- тест: два concurrent transaction Bundle с одинаковым `If-None-Exist`;
- ожидаем ровно один create или корректный conflict по FHIR semantics.

Риск:

- Больше read load на primary, но это correctness path.

## P1: Search и индексы от реальной нагрузки

### 4. Index Advisor как read-only менеджер рекомендаций

Файлы:

- `crates/octofhir-server/src/operations/db_console_api.rs`
- `crates/octofhir-server/src/server.rs`
- будущая UI-вкладка в `ui/src/pages/db-console`

Что сделать:

- Принимать реальные FHIR request shapes.
- Показывать:
  - какие built-in projections уже должны покрывать запрос;
  - где нужен keyset index;
  - где нужен token/string/quantity projection;
  - какие большие индексы выглядят подозрительно неиспользуемыми.
- Не выполнять DDL автоматически.

Проверка:

- unit tests на разбор запросов:
  - `/fhir/Observation?subject=Patient/1&code=x`
  - `/fhir/Patient?name:contains=ivan`
  - `/fhir/Observation?date=ge2024`

Риск:

- Advisor не должен советовать неправильные expression indexes по FHIRPath без
  доказанного SQL mapping.

### 5. Token projection для hot token search

Файлы:

- migration в `crates/octofhir-db-postgres/migrations`
- `crates/octofhir-db-postgres/src/search_index.rs`
- `crates/octofhir-core/src/search_index.rs`
- `crates/octofhir-search/src/types/mod.rs`

Кандидатная таблица:

```sql
search_idx_token(
  resource_type text,
  resource_id text,
  param_code text,
  system text,
  code text,
  display text
)
```

Индексы:

```sql
(resource_type, param_code, system, code, resource_id)
(resource_type, param_code, code, resource_id)
```

Что покрывает:

- `Observation?code=system|code`
- `Condition?code=...`
- `MedicationRequest?status=active`
- `Patient?identifier=system|value`

Что не делать:

- Не индексировать все token-like JSONB поля без SearchParameter registry.

Проверка:

- сравнить JSONB containment vs token projection на 100k-1M rows.
- измерить write amplification: rows в projection на один resource.

### 6. Ограничить include/revinclude fanout

Файлы:

- `crates/octofhir-search/src/include.rs`
- `crates/octofhir-db-postgres/src/queries/search.rs`
- `crates/octofhir-server/src/handlers.rs`

Что сделать:

- max include specs;
- max included resources;
- max recursive depth;
- OperationOutcome при превышении лимита.

Проверка:

- `Observation?_include=Observation:subject`;
- `Patient?_revinclude=Observation:subject`;
- synthetic patient с большим числом observations.

Риск:

- Нужно явно документировать лимиты, иначе пользователи увидят truncation как bug.

## P2: Write path и bulk

### 7. Bulk import batch upsert per resource type

Файлы:

- `crates/octofhir-server/src/operations/bulk/import.rs`
- `crates/octofhir-db-postgres/src/search_index.rs`

Что сделать:

- Группировать NDJSON batch по `resourceType`.
- Один batch upsert через `UNNEST` на resource table.
- Один batch flush search projections.
- Сохранить per-line error reporting.

Проверка:

- локально 100k NDJSON resources;
- метрики: resources/sec, WAL bytes/resource, DB roundtrips.

Риск:

- Сложнее частичные ошибки в batch.

### 8. Search index consistency mode

Файлы:

- `crates/octofhir-db-postgres/src/storage.rs`
- `crates/octofhir-db-postgres/src/index_writer.rs`
- `crates/octofhir-server/src/config.rs`

Что сделать:

- Добавить режимы:
  - `async`: текущий быстрый режим;
  - `strict`: resource write и projection write в одной DB transaction;
  - `outbox`: commit resource + durable index job, worker repairable.

Проверка:

- failpoint на index write;
- в strict mode successful write не должен оставлять stale projection.

Риск:

- Strict mode увеличит write latency.

## P3: Структурные улучшения без тяжелой инфраструктуры

### 9. History partitioning по времени или txid

Файлы:

- `crates/octofhir-db-postgres/src/schema.rs`
- migrations
- `crates/octofhir-db-postgres/src/queries/history.rs`

Что сделать:

- Новые history tables создавать partitioned.
- Current resource table оставить без partitioning.
- Добавить retention/archive hooks позднее.

Проверка:

- update-heavy synthetic dataset;
- `type/_history` по recent window и full history.

Риск:

- Migration existing history tables требует аккуратного backfill.

### 10. GraphQL loaders настоящими batch-read

Файлы:

- `crates/octofhir-graphql/src/loaders`
- `crates/octofhir-db-postgres/src/queries/crud.rs`

Что сделать:

- Для одного resource type читать `WHERE id = ANY($1)`.
- Не делать последовательный read per id.

Проверка:

- GraphQL query с 100 references;
- DB roundtrips должны стать O(resource types), не O(resources).

## Не делать сейчас

1. Не вводить шардирование.
2. Не переносить все ресурсы в одну таблицу.
3. Не добавлять expression index на каждый SearchParameter.
4. Не делать 100GB benchmark как обязательный gate.
5. Не добавлять автоматическое выполнение DDL из advisor-а.

## Минимальный benchmark pack

Достаточно локально:

- 10k rows: correctness/smoke.
- 100k rows: planner behavior.
- 1M rows для `Observation`, если есть место.

Сценарии:

- read by id;
- search by `_lastUpdated`;
- search by `date`;
- search by `subject`;
- search by `code`;
- `_include` and `_revinclude`;
- bulk import 100k NDJSON.

Команда для SQL-проверки:

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT ...
```

Фиксировать:

- latency p50/p95;
- rows scanned;
- shared read/hit;
- WAL bytes for writes;
- index size before/after.

