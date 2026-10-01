Результаты: [REPORT.md](REPORT.md), [results.jsonl](results.jsonl), [SQL-SAMPLES.md](SQL-SAMPLES.md).

Анализ выполнен в рабочем дереве на HEAD `7eccf11bc3fe4f38ff334da68c14aa3d97331320` с уже существовавшими локальными изменениями. [source-sha256.json](source-sha256.json) фиксирует исследованные исходники и dependency inputs на момент завершения. Соседние path dependencies и Docker image digest не зафиксированы: перед релизным сравнением нужны pinned зависимости и образ.

Для повторения SQL probe из корня репозитория:

```sh
# Требуется успешная сборка текущих search/postgres crates и dev-dependencies.
# Аудит выполнил её через:
cargo test -p octofhir-db-postgres --test search_explain_test --offline --no-run

# Только компиляция диагностической программы, без Docker/БД:
AUDIT_BUILD_ONLY=1 python3 internal-docs/audits/search-2026-09-09/run.py

# SQL/semantic probe: новый одноразовый postgres:18, синтетические данные.
# results.jsonl и plan JSON в этой папке будут перезаписаны.
AUDIT_ROWS=20000 python3 internal-docs/audits/search-2026-09-09/run.py
```

`run.py` линкует probe с последними rlib/dylib из `target/debug/deps`. **После изменения исходников сначала пересобрать зависимости**; иначе измеряться будет старая сборка. При несовместимых artifacts rustc завершится ошибкой. Cargo-команда без `--locked` может обновить lockfile; текущая проверка `--locked` не прошла, подробности в [build.log](build.log). Сам run.py Cargo не запускает и lockfile не меняет.

Probe не использует DATABASE_URL или octofhir.toml: Postgres URL получается исключительно от созданного им testcontainer. Не нужно менять рабочие БД. Testcontainers должен иметь доступ к локальному Docker daemon; контейнер удаляется при завершении. Успешный итоговый запуск имеет 72 JSONL records и exit code 0. [probe.stderr](probe.stderr) содержит **ожидаемый пойманный panic** от отдельного теста chained Unicode; это подтверждение дефекта, программа после него завершилась нормально.

Все `*_candidate` — диагностические SQL для ограниченных fixture shapes. Они не готовы для универсального production search и не являются внедрёнными исправлениями. `composite` использует hand-registered fixture из существующего теста; это не проверка полного registry R4. Дополнительные quantity/code indexes создаются только в одноразовом контейнере после основной серии замеров.

Для HTTP измерений отдельно запустить **текущую release-сборку OctoFHIR в локальном изолированном окружении**, загрузить данные с соответствующими identifiers/family/references либо задать значения переменными. Probe уничтожает свою БД и не подготавливает сервер для k6. HTTP-скрипт read-only; наличие данных и правильность множества результатов проверяются перед нагрузкой. Аутентификация не отключается: TOKEN берётся из окружения, если сервер его требует.

```sh
# Только проверка k6-сценария: выполнена в этом аудите, запросов к серверу не делает.
k6 inspect -e BASE_URL=http://127.0.0.1:8888/fhir internal-docs/audits/search-2026-09-09/http-search.js

# Пример нагрузки после подготовки локального сервера/данных (в аудите НЕ запускался):
BASE_URL=http://127.0.0.1:8888/fhir VUS=8 DURATION=90s \
  k6 run --summary-export=/tmp/search-http-summary.json \
  internal-docs/audits/search-2026-09-09/http-search.js
```

Другие настройки данных: `PATIENT_ID`, `FAMILY`, `MRN_SYSTEM`, `MRN`, `CODE`. Скрипт намеренно принимает только localhost/127.0.0.1 URL с `/fhir`. Проверки HTTP 200/searchset/page size — smoke, не замена семантическим assertions. По case доступны p50/p95/p99; общий request rate/error rate — в k6 summary. Не смешивать результаты разных fixture, количества VUs и режимов кеширования.

Отдельно сохранён [legacy-explain.log](legacy-explain.log): штатный ignored smoke падает на PostgreSQL 11 до выполнения representative SQL. Product code, рабочая схема БД и индексная конфигурация этим аудитом не исправлялись.
