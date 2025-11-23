pub mod chaining;
pub mod common;
pub mod engine;
pub mod include;
pub mod loader;
pub mod parameters;
pub mod parser;
pub mod registry;
pub mod reverse_chaining;
pub mod sql_builder;
pub mod types;

pub use common::register_common_parameters;
pub use engine::{EngineError, SearchConfig, SearchEngine};
pub use loader::{LoaderError, load_search_parameters, parse_search_parameter};
pub use parameters::{
    SearchModifier, SearchParameter, SearchParameterDefinition, SearchParameterType,
    SearchParameters,
};
pub use parser::{ParsedParameters, SearchParameterParser};
pub use registry::SearchParameterRegistry;
pub use sql_builder::{
    SqlBuilder, SqlBuilderError, SqlParam, build_jsonb_accessor, fhirpath_to_jsonb_path,
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
pub use include::{
    IncludeError, IncludeParam, extract_includes, extract_revincludes, is_include_parameter,
    is_revinclude_parameter, parse_include, parse_revinclude,
};
pub use reverse_chaining::{
    ReverseChainParameter, ReverseChainingError, build_reverse_chain_search,
    is_reverse_chain_parameter, parse_reverse_chain,
};
