use proc_macro2::TokenStream;
use quote::quote;

struct DeriveMigrationMeta {
    ident: syn::Ident,
}

impl DeriveMigrationMeta {
    fn new(input: syn::DeriveInput) -> Self {
        let ident = input.ident;

        DeriveMigrationMeta { ident }
    }

    fn expand(&self) -> TokenStream {
        let ident = &self.ident;

        quote!(
            #[automatically_derived]
            impl vectorctl::MigrationMeta for #ident {
                fn name(&self) -> vectorctl::MigrationName {
                    vectorctl::get_file_stem(file!()).into()
                }

                fn revision(&self) -> vectorctl::Revision {
                    REVISION
                }
            }
        )
    }
}

pub fn expand_derive_migration_meta(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    Ok(DeriveMigrationMeta::new(input).expand())
}
