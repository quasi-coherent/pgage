//! # pgage-derive
//!
//! Derive macros for [`pgage`].
//!
//! [`pgage`]: https://crates.io/crates/pgage
use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod entity;

#[proc_macro_derive(Entity, attributes(pgage))]
pub fn entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    entity::expand_entity(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
