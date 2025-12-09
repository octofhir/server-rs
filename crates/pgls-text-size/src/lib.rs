// This crate is derived from Supabase postgres-language-server
// https://github.com/supabase-community/postgres-language-server
// Licensed under MIT License
// Copyright (c) 2023 Philipp Steinrötter

//! Newtypes for working with text sizes/ranges in a more type-safe manner.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, missing_docs)]

mod range;
mod size;
mod text_range_replacement;
mod traits;

pub use crate::{
    range::TextRange,
    size::TextSize,
    text_range_replacement::{TextRangeReplacement, TextRangeReplacementBuilder},
    traits::TextLen,
};

#[cfg(target_pointer_width = "16")]
compile_error!("text-size assumes usize >= u32 and does not work on 16-bit targets");
