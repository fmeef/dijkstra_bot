use std::collections::HashMap;

use convert_case::{Case, Casing};
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, LitStr, Token,
};

static STRINGS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/strings");

lazy_static! {
    static ref LOCALE: Locale = get_locale();
}

#[derive(Deserialize)]
struct Strings {
    #[serde(flatten)]
    strings: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Locale {
    #[serde(flatten)]
    langs: HashMap<String, Strings>,
}

fn get_locale() -> Locale {
    STRINGS_DIR.files().fold(
        Locale {
            langs: HashMap::new(),
        },
        |mut acc, file| {
            let lang: Strings = serde_yaml::from_reader(file.contents()).unwrap();
            acc.langs.insert(
                file.path()
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
                lang,
            );
            acc
        },
    )
}

struct LocaleInput {
    st: LitStr,
    format: Punctuated<Expr, Token![,]>,
}

impl Parse for LocaleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let st: LitStr = input.parse()?;
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        Ok(Self {
            st,
            format: input.parse_terminated(Expr::parse)?,
        })
    }
}

struct LangLocaleInput {
    lang: Expr,
    st: LitStr,
    format: Punctuated<Expr, Token![,]>,
}

impl Parse for LangLocaleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lang: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let st: LitStr = input.parse()?;
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        Ok(Self {
            lang,
            st,
            format: input.parse_terminated(Expr::parse)?,
        })
    }
}

#[proc_macro]
pub fn get_langs(_: TokenStream) -> TokenStream {
    let names = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| {
            let name = thing.unwrap().to_str().unwrap();
            let v = name.to_case(Case::UpperCamel);
            let v = format_ident!("{}", v);
            quote! {
                #[sea_orm(string_value = #name)]
                #v
            }
        });

    let into = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| {
            let name = thing.unwrap().to_str().unwrap();
            let v = name.to_case(Case::UpperCamel);
            let v = format_ident!("{}", v);

            quote! {
                Self::#v => #name.to_owned()
            }
        });

    let from = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| {
            let name = thing.unwrap().to_str().unwrap();
            let v = name.to_case(Case::UpperCamel);
            let v = format_ident!("{}", v);

            quote! {
                #name => Self::#v
            }
        });

    let vnames = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| thing.unwrap().to_str().unwrap().to_case(Case::UpperCamel))
        .map(|v| format_ident!("{}", v))
        .map(|v| {
            quote! {
                Lang::#v
            }
        });

    let res = quote! {
        pub mod langs {
            use serde::{Serialize, Deserialize};
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
            #[sea_orm(rs_type = "String", db_type = "String(Some(16))")]
            pub enum Lang {
                #( #names )*,
                #[sea_orm(string_value = "Invalid")]
                Invalid
            }

            pub fn get_langs() -> Vec<Lang> {
                vec![ #( #vnames ),*]
            }

            impl Lang {
                pub fn from_code<T: AsRef<str>>(code: T) ->  Self {
                    match code.as_ref() {
                        #( #from )*,
                        _ => Self::Invalid
                    }
                }

                pub fn into_code(self) -> String {
                    match self {
                        #( #into )*,
                        Self::Invalid => "invalid".to_string()
                    }
                }
            }
        }
    };
    TokenStream::from(res)
}

#[proc_macro]
pub fn rformat(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LocaleInput);
    let key = input.st;
    let format = LOCALE
        .langs
        .get("en")
        .expect("invalid language")
        .strings
        .get(&key.value())
        .expect("invalid resource");

    let idents = input.format.iter();
    let res = quote! {
        format!(#format, #( #idents ),*)
    };
    TokenStream::from(res)
}

#[proc_macro]
pub fn rlformat(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let language = input.lang;
    let format = LOCALE
        .langs
        .get("en")
        .expect("invalid language")
        .strings
        .get(&key.value())
        .expect("invalid resource");

    let arms = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| thing.unwrap().to_str().unwrap().to_case(Case::UpperCamel))
        .map(|v| format_ident!("{}", v))
        .map(|v| {
            let idents = input.format.iter();
            quote! {
                #v => format!(#format, #( #idents ),*)
            }
        });
    let res = quote! {
        match #language {
            #( #arms )*,
            Invalid => "invalid".to_owned()
        }
    };
    TokenStream::from(res)
}
