//! The derive macros of starmap. `#[derive(Env)]` embeds an application's
//! `starmap.toml` and generates `Env::load()`, and `#[derive(Enumeration)]`
//! reads a variable into an enum. starmap re-exports both behind its
//! `derive` feature, which documents them.
//!
//! The generated code names starmap as `::starmap`, so an application
//! depends on starmap under that name.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod enumeration;
mod env;

/// Derives `starmap::Env` for a struct whose fields each read one variable.
/// starmap's re-export documents the attributes.
#[proc_macro_derive(Env, attributes(starmap))]
pub fn derive_env(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    env::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `starmap::Enumeration` for an enum of unit variants. starmap's
/// re-export documents the attributes.
#[proc_macro_derive(Enumeration, attributes(starmap))]
pub fn derive_enumeration(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    enumeration::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
