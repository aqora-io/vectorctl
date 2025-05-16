extern crate proc_macro;

use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

mod derives;

#[proc_macro_derive(DeriveMigrationName)]
pub fn derive_migration_name(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derives::expand_derive_migration_name(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
