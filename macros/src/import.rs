use std::{fs::read_dir, path::PathBuf};

use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::LitStr;

pub struct PathBufWrapper(std::path::PathBuf);
pub struct PathList(Vec<PathBufWrapper>);

impl ToTokens for PathBufWrapper {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let iter = self.0.iter().map(|v| v.to_str().unwrap().to_string());
        tokens.extend(quote! {
            [ #( #iter ),* ].iter().collect()
        })
    }
}

impl ToTokens for PathList {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let iter = self.0.iter();
        tokens.extend(quote! {
            {
            let mut _pathvec = Vec::<std::path::PathBuf>::new();
            #(
                let _pathitem: std::path::PathBuf = #iter;
                _pathvec.push(_pathitem);
            )*

            _pathvec
            }
        })
    }
}

fn get_dir(dir: &LitStr) -> PathBufWrapper {
    PathBufWrapper(PathBuf::from(dir.value()))
}

fn get_module_list(dir: &PathBufWrapper) -> Vec<Ident> {
    read_dir(&dir.0)
        .unwrap()
        .map(|d| d.unwrap())
        .filter(|d| {
            let name = d.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && (name.ends_with(".rs") || d.file_type().unwrap().is_dir())
        })
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .map(|name| name.trim_end_matches(".rs").to_owned())
        .filter(|name| name != "main" && name != "mod")
        .map(|name| quote::format_ident!("{}", name))
        .collect()
}

pub(crate) fn autoimport(input: TokenStream) -> TokenStream {
    let input: LitStr = syn::parse2(input).unwrap();
    let dir = get_dir(&input);
    let mods = get_module_list(&dir).into_iter();
    let output = quote! {
        #( mod #mods; )*
    };
    output
}
