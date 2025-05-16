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
            impl qdrant_tools_migration::MigrationMeta for #ident {
                fn id(&self) -> qdrant_tools_migration::migrator::MigrationId {
                    qdrant_tools_migration::get_file_stem(file!()).into()
                }

                fn message(&self) -> String {
                    file!().to_string()
                }
            }
        )
    }
}

/// Method to derive a MigrationName
pub fn expand_derive_migration_name(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    Ok(DeriveMigrationName::new(input).expand())
}
