use core::panic;
use std::{collections::HashMap, sync::RwLock};

use convert_case::{Case, Casing};
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use serde::Deserialize;
use syn::{
    braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    token::{Brace, Comma},
    Expr, LitStr, Token,
};

static STRINGS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/strings");

lazy_static! {
    static ref LOCALE: RwLock<Locale> = RwLock::new(get_locale());
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
    ctx: Expr,
    st: LitStr,
    format: Punctuated<Expr, Token![,]>,
}

impl Parse for LangLocaleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ctx: Expr = input.parse()?;
        let _: Token![,] = input.parse()?;
        let st: LitStr = input.parse()?;
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        Ok(Self {
            ctx,
            st,
            format: input.parse_terminated(Expr::parse)?,
        })
    }
}

struct InlineRow {
    lang: LitStr,
    yaml: LitStr,
}

struct InlineInput(Punctuated<InlineRow, Token![,]>);

impl Parse for InlineRow {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        let _: Brace = braced!(content in input);
        let lang: LitStr = content.parse()?;
        let _: Token![=>] = content.parse()?;
        let yaml: LitStr = content.parse()?;
        Ok(Self { lang, yaml })
    }
}

impl Parse for InlineInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self(input.parse_terminated(InlineRow::parse)?))
    }
}

#[proc_macro]
pub fn inline_lang(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as InlineInput);
    let mut locale = LOCALE.write().unwrap();
    for row in input.0 {
        let s: Strings = serde_yaml::from_str(&row.yaml.value()).expect("invalid yaml");
        if let Some(lang) = locale.langs.get_mut(&row.lang.value()) {
            lang.strings.extend(s.strings.into_iter());
        } else {
            locale.langs.insert(row.lang.value(), s);
        }
    }
    let res = quote! {};
    TokenStream::from(res)
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
pub fn string_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LocaleInput);
    let key = input.st;
    let locale = LOCALE.read().unwrap();
    let format = locale
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
pub fn textentity_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let ctx = input.ctx;
    let args = input.format;
    let m = get_entity_match(&ctx, key, args);

    let res = quote! {
        {
            let mut builder = crate::tg::markdown::EntityMessage::new(#ctx.try_get()?.chat.get_id());
            #m;
            builder
        }
    };
    TokenStream::from(res)
}

#[proc_macro]
pub fn entity_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let ctx = input.ctx;
    let args = input.format;
    let m = get_entity_match(&ctx, key, args);

    let res = quote! {
        {
            let mut builder = crate::tg::markdown::EntityMessage::new(#ctx.try_get()?.chat.get_id());
            #m;
            builder
        }
    };
    TokenStream::from(res)
}

#[proc_macro]
pub fn message_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let ctx = input.ctx;
    let args = input.format;
    let m = get_match(&ctx, key, args);

    let res = quote! {
        {
            crate::statics::TG.client()
                .build_send_message(#ctx.try_get()?.chat.get_id(), &#m)
        }
    };
    TokenStream::from(res)
}

fn get_entity_match(ctx: &Expr, key: LitStr, args: Punctuated<Expr, Comma>) -> impl ToTokens {
    let locale = LOCALE.read().unwrap();
    let mut format = locale
        .langs
        .get("en")
        .expect("invalid language")
        .strings
        .get(&key.value())
        .expect("invalid resource")
        .split("{}")
        .collect::<Vec<&str>>();

    let last = format.pop().expect("empty format");
    if format.len() != args.len() {
        panic!("wrong number of arguments {:?} {}", format, args.len());
    }

    let arms = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem())
        .map(|thing| thing.unwrap().to_str().unwrap().to_case(Case::UpperCamel))
        .map(|v| format_ident!("{}", v))
        .map(|v| {
            let idents = args.iter();
            quote! {
                #v => builder.builder() #(.text(#format).regular(#idents.into()))*.text(#last).build()
            }
        });
    quote! {
        match #ctx.lang() {
            #( #arms )*,
            Invalid => ("invalid", &(*crate::tg::markdown::EMPTY_ENTITIES))
        }
    }
}

fn get_match(language: &Expr, key: LitStr, args: Punctuated<Expr, Comma>) -> impl ToTokens {
    let locale = LOCALE.read().unwrap();
    let format = locale
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
            let idents = args.iter();
            quote! {
                #v => format!(#format, #( #idents ),*)
            }
        });
    quote! {
        match #language {
            #( #arms )*,
            Invalid => "invalid".to_owned()
        }
    }
}

#[proc_macro]
pub fn lang_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let ctx = input.ctx;
    let args = input.format;
    let m = get_match(&ctx, key, args);
    TokenStream::from(quote! { #m })
}
