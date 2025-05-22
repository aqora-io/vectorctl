use std::{env, fs, path::Path};

use lazy_regex::regex;
use proc_macro2::{Span, TokenStream};
use quote::quote;

struct DeriveAutoMigrator {
    ident: syn::Ident,
}

impl DeriveAutoMigrator {
    pub fn new(input: syn::DeriveInput) -> Self {
        Self { ident: input.ident }
    }

    pub fn expand(&self) -> TokenStream {
        let root = env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set at macro expension time");

        let root_path = Path::new(&root).join("src");

        let re = regex!(r"^version.*\.rs$");

        let mut stems: Vec<String> = fs::read_dir(root_path)
            .expect("Failed to read directory. Check permission")
            .filter_map(|item| {
                let path = item.ok()?.path();
                path.file_name()
                    .and_then(|stem| stem.to_str())
                    .filter(|name| re.is_match(name))
                    .map(|name| name.trim_end_matches(".rs").to_owned())
            })
            .collect();
        stems.sort();

        let idents: Vec<syn::Ident> = stems
            .iter()
            .map(|stem| syn::Ident::new(stem, Span::call_site()))
            .collect();

        let ident = &self.ident;

        quote! {
            #[automatically_derived]
            use vectorctl::MigrationTrait;
            pub use vectorctl::MigratorTrait;

            #( mod #idents; )*

            #[async_trait::async_trait]
            impl MigratorTrait for #ident {
                fn migrations() -> Vec<Box<dyn MigrationTrait>> {
                    vec![ #( Box::new(#idents::Migration) ),* ]
                }
            }
        }
    }
}

pub fn expand_derive_auto_migrator(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    Ok(DeriveAutoMigrator::new(input).expand())
}
