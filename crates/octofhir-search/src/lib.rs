pub mod chaining;
pub mod common;
pub mod config_watcher;
pub mod include;
pub mod loader;
pub mod parameters;
pub mod params_converter;
pub mod parser;
pub mod query_cache;
pub mod registry;
pub mod reloadable;
pub mod reverse_chaining;
pub mod sql_builder;
pub mod terminology;
pub mod types;

pub use common::register_common_parameters;
pub use loader::{LoaderError, load_search_parameters, parse_search_parameter};
pub use parameters::{
    SearchModifier, SearchParameter, SearchParameterDefinition, SearchParameterType,
    SearchParameters,
};
pub use parser::{ParsedParameters, SearchParameterParser};
pub use registry::SearchParameterRegistry;
pub use sql_builder::{
    // Fluent query builder
    BuiltQuery,
    ChainJoin,
    FhirQueryBuilder,
    IncludeSpec,
    JsonbPath,
    Operator,
    Pagination,
    QueryMode,
    RevIncludeSpec,
    SearchCondition,
    SortOrder,
    SortSpec,
    // SQL builder utilities
    SqlBuilder,
    SqlBuilderError,
    SqlParam,
    SqlValue,
    build_jsonb_accessor,
    escape_identifier,
    fhirpath_to_jsonb_path,
};
pub use types::{
    DateRange, build_date_search, build_human_name_search, build_identifier_search,
    build_number_search, build_period_search, build_quantity_search, build_string_search,
    build_token_search, dispatch_search, parse_date_range, parse_token_value,
};

// Chaining and includes
pub use chaining::{
    ChainLink, ChainedParameter, ChainingError, build_chained_search, is_chained_parameter,
    parse_chained_parameter,
};
pub use config_watcher::{
    ConfigCallback, ConfigWatcher, ReloadableTerminologyProvider, WatcherConfig, WatcherError,
    WatcherHandle, watch_and_reload,
};
pub use include::{
    IncludeError, IncludeParam, extract_includes, extract_revincludes, is_include_parameter,
    is_revinclude_parameter, parse_include, parse_revinclude,
};
pub use query_cache::{
    CacheError, CacheStatsSnapshot, ParamPosition, ParamValueType, PreparedQuery, QueryCache,
    QueryCacheKey, QueryParamKey,
};
pub use reloadable::{ReloadableSearchConfig, SearchConfig, SearchOptions};
pub use reverse_chaining::{
    ReverseChainParameter, ReverseChainingError, build_reverse_chain_search,
    is_reverse_chain_parameter, parse_reverse_chain,
};
pub use terminology::{
    CacheStats, ExpansionResult, HierarchyDirection, HybridTerminologyProvider, TerminologyConfig,
    TerminologyError,
};

// SearchParams to query builder conversion
pub use params_converter::{ConvertedQuery, build_query_from_params, parse_query_string};
