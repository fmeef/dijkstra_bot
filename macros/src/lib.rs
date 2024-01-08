use std::{collections::HashMap, sync::RwLock};
mod import;
use convert_case::{Case, Casing};
use import::autoimport;
use include_dir::{include_dir, Dir};
use lazy_static::lazy_static;
use proc_macro::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};
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

static STRINGS_DIR: Dir<'_> = include_dir!("$DIJKSTRA_STRINGS_DIR");

lazy_static! {
    static ref LOCALE: RwLock<Locale> = RwLock::new(get_locale());
    static ref STRINGS: Vec<String> = get_sorted_filenames();
}

fn get_sorted_filenames() -> Vec<String> {
    let mut v = STRINGS_DIR
        .files()
        .map(|f| f.path().file_stem().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<String>>();
    v.sort();
    v
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
pub fn discover_mods(tokens: TokenStream) -> TokenStream {
    let v = parse_macro_input!(tokens as LitStr);
    let out = autoimport(v.value());
    TokenStream::from(out)
}

#[proc_macro]
pub fn get_langs(_: TokenStream) -> TokenStream {
    let names = STRINGS.iter().map(|name| {
        let v = name.to_case(Case::UpperCamel);
        let v = format_ident!("{}", v);
        quote! {
            #[sea_orm(string_value = #name)]
            #v
        }
    });

    let mnames = STRINGS.iter().map(|name| {
        let v = name.to_case(Case::UpperCamel);
        let v = format_ident!("{}", v);
        quote! {
             Self::#v
        }
    });

    let into = STRINGS.iter().map(|name| {
        let v = name.to_case(Case::UpperCamel);
        let v = format_ident!("{}", v);

        quote! {
            Self::#v => #name
        }
    });

    let from = STRINGS.iter().map(|name| {
        let v = name.to_case(Case::UpperCamel);
        let v = format_ident!("{}", v);

        quote! {
            #name => Self::#v
        }
    });

    let vnames = STRINGS
        .iter()
        .map(|thing| thing.to_case(Case::UpperCamel))
        .map(|v| format_ident!("{}", v))
        .map(|v| {
            quote! {
                Lang::#v
            }
        });

    let ids: Vec<usize> = (0..STRINGS.len()).collect();

    let res = quote! {
        #[doc = "Autogenerated language files, edit the files in ./strings to change these"]
        pub mod langs {
            use serde::{Serialize, Deserialize};
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
            #[sea_orm(rs_type = "String", db_type = "String(Some(16))")]
            pub enum Lang {
                #( #names ),*,
                #[sea_orm(string_value = "Invalid")]
                Invalid
            }

            pub fn get_langs() -> Vec<Lang> {
                vec![ #( #vnames ),*]
            }

            impl Lang {
                pub fn get_id(&self) -> Option<usize> {
                    match self {
                        #( #mnames => Some(#ids) ),*,
                        Self::Invalid => None
                    }
                }

                pub fn from_code<T: AsRef<str>>(code: T) ->  Self {
                    match code.as_ref() {
                        #( #from ),*,
                        _ => Self::Invalid
                    }
                }

                pub fn lang<'a>(&'a self) -> &'a Self {
                    &self
                }

                pub fn into_code(self) -> &'static str {
                    match self {
                        #( #into ),*,
                        Self::Invalid => "invalid"
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

fn get_current_crate() -> impl ToTokens {
    let c = crate_name("dijkstra").expect("dijkstra crate not found");
    match c {
        FoundCrate::Itself => quote! { crate },
        FoundCrate::Name(name) => {
            let name = format_ident!("{}", name);
            quote! { :: #name }
        }
    }
}

#[proc_macro]
pub fn textentity_fmt(tokens: TokenStream) -> TokenStream {
    let input = parse_macro_input!(tokens as LangLocaleInput);
    let key = input.st;
    let ctx = input.ctx;
    let args = input.format;
    let m = get_entity_match(&ctx, key, args);
    let c = get_current_crate();
    let res = quote! {
        {
            let mut builder = #c ::tg::markdown::EntityMessage::new(#ctx.try_get()?.chat.get_id());
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

    let c = get_current_crate();
    let res = quote! {
        {
            let mut builder = #c ::tg::markdown::EntityMessage::new(#ctx.try_get()?.chat.get_id());
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
    let c = get_current_crate();
    let res = quote! {
        {
            #c ::statics::TG.client()
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
    let c = get_current_crate();
    let arms = STRINGS.iter()
          .map(|v| (v.to_case(Case::UpperCamel), v))
        .map(|(v,u)| {
            let v = format_ident!("{}", v);
            let idents = args.iter();
            if let Some(format) = locale.langs.get(u.as_str()).unwrap().strings.get(&key.value()) {
                let format = format.split("{}").collect::<Vec<&str>>();
                quote! {
                    #c ::langs::Lang::#v => builder.builder #(.text(#format).regular_fmt(#idents.into()))*.text(#last).build()
                }
            } else {
                quote! {
                    #c ::langs::Lang::#v => builder.builder #(.text(#format).regular_fmt(#idents.into()))*.text(#last).build()
                }
            }
        });

    let c = get_current_crate();
    quote! {
        match #ctx.lang() {
            #( #arms ),*,
            #c ::langs::Lang::Invalid => ("invalid", &(* #c ::tg::markdown::EMPTY_ENTITIES))
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

    let c = get_current_crate();
    let arms = STRINGS
        .iter()
        .map(|thing| (thing, thing.to_case(Case::UpperCamel)))
        .map(|(u, v)| (u, format_ident!("{}", v)))
        .map(|(u, v)| {
            let idents = args.iter();
            if let Some(format) = locale.langs.get(u).unwrap().strings.get(&key.value()) {
                quote! {
                    #c ::langs::Lang::#v => format!(#format, #( #idents ),*)
                }
            } else {
                quote! {

                     #c ::langs::Lang::#v => format!(#format, #( #idents ),*)
                }
            }
        });

    let c = get_current_crate();

    quote! {
        match #language.lang() {
            #( #arms ),*,
            #c ::langs::Lang::Invalid => "invalid".to_owned()
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

#[proc_macro_attribute]
pub fn update_handler(_: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemFn);
    let name = &input.sig.ident;

    let c = get_current_crate();

    quote! {
        #input
        pub mod update_handler {
            pub async fn handle_update(context: & #c ::tg::command::Context) -> #c ::util::error::Result<()> {
                super:: #name (context).await
            }
        }
    }.into()
}
