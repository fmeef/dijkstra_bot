use std::fs::read_dir;

use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
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

fn glob_modules<T: AsRef<str>>(spec: T) -> Vec<Ident> {
    glob::glob(spec.as_ref())
        .expect("invalid glob pattern")
        .map(|v| v.expect("glob error"))
        .map(|v| PathBufWrapper(v))
        .map(|v| get_module_list(&v))
        .flat_map(|v| v.into_iter())
        .collect()
}

fn get_module_list(dir: &PathBufWrapper) -> Vec<Ident> {
    read_dir(&dir.0)
        .expect("directory does not exist")
        .map(|d| d.expect("entry does not exist"))
        .filter(|d| {
            let name = d.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.')
                && (name.ends_with(".rs")
                    || d.file_type()
                        .expect(&format!("file type for {} does not exist", name))
                        .is_dir())
        })
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .map(|name| name.trim_end_matches(".rs").to_owned())
        .filter(|name| name != "main" && name != "mod")
        .map(|name| quote::format_ident!("{}", name))
        .collect()
}

pub fn autoimport<T: AsRef<str>>(input: T) -> TokenStream {
    let module_globs = glob_modules(input);
    assert!(module_globs.len() > 0);
    let mods = module_globs.clone().into_iter();
    let updates = module_globs.clone().into_iter();
    let funcs = module_globs.iter();
    let modules = module_globs.iter();
    let output = quote! {
        #( mod #mods; )*
        use crate::util::string::Speak;
        pub fn get_migrations() -> ::std::vec::Vec<::std::boxed::Box<dyn ::sea_orm_migration::prelude::MigrationTrait>> {
            let mut v = ::std::vec::Vec::<::std::boxed::Box<dyn ::sea_orm_migration::prelude::MigrationTrait>>::new();
            #(
                v.append(&mut #funcs::get_migrations());
            )*
            v
        }

        pub fn get_metadata() -> ::std::vec::Vec<crate::metadata::Metadata> {
            vec![#(
                 (*#modules::METADATA).clone()
            ),*]
        }

        pub async fn process_updates(
            update: ::botapi::gen_types::UpdateExt,
            helps: ::std::sync::Arc<crate::tg::client::MetadataCollection>
            ) -> crate::util::error::Result<()> {
            match crate::tg::command::Context::get_context(&update).await {
                Ok(cmd) => {

                    let help = if let Some((cmd, _, args, message, lang)) = cmd.as_ref().map(|v| v.cmd()).flatten() {
                         match cmd {
                            "help" => crate::tg::client::show_help(message, helps).await,
                            "start" => match args.args.first().map(|a| a.get_text()) {
                                Some("help") => {
                                    crate::tg::client::show_help(message, helps).await?;
                                    Ok(true)
                                }

                                None => {
                                    message.reply(macros::lang_fmt!(lang, "startcmd")).await?;
                                    Ok(true)
                                }
                                _ => Ok(false),
                            },
                            _ => Ok(false),
                        }
                    } else {
                       Ok(false)
                    };
                    match help {
                        Ok(false) => {#(
                            if let Err(err) = #updates::handle_update(&update, &cmd).await {
                                err.record_stats();
                                match err.get_message().await {
                                    Err(err) => {
                                        log::error!("failed to send error message: {}, what the FLOOP", err);
                                        err.record_stats();
                                    }
                                    Ok(v) => if ! v {
                                       log::error!("handle_update {} error: {}", #updates::METADATA.name, err);
                                    }
                                }
                            }
                        )*}
                       Ok(true) => (),
                      Err(err)  => log::error!("failed help {}", err)
                    }

                }
                Err(err) => err.record_stats(),
            }
            Ok(())
        }
    };
    output
}
