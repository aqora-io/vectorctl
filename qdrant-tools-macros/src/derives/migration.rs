use proc_macro2::TokenStream;
use quote::quote;

struct DeriveMigrationName {
    ident: syn::Ident,
}

impl DeriveMigrationName {
    fn new(input: syn::DeriveInput) -> Self {
        let ident = input.ident;

        DeriveMigrationName { ident }
    }

    fn expand(&self) -> TokenStream {
        let ident = &self.ident;

        quote!(
            #[automatically_derived]
            impl qdrant_tools_migration::MigrationName for #ident {
                fn name(&self) -> &str {
                    qdrant_tools_migration::get_file_stem(file!())
                }
            }
        )
    }
}

/// Method to derive a MigrationName
pub fn expand_derive_migration_name(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    Ok(DeriveMigrationName::new(input).expand())
}
