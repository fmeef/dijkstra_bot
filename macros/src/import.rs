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

fn glob_modules<T: AsRef<str>>(spec: T, file_ext: &str) -> Vec<String> {
    glob::glob(spec.as_ref())
        .expect("invalid glob pattern")
        .map(|v| v.expect("glob error"))
        .map(PathBufWrapper)
        .map(|v| {
            read_dir(v.0)
                .expect("directory does not exist")
                .map(|d| d.expect("entry does not exist"))
                .filter(|d| {
                    let name = d.file_name();
                    let name = name.to_string_lossy();
                    !name.starts_with('.')
                        && (name.ends_with(file_ext)
                            || (d
                                .file_type()
                                .unwrap_or_else(|_| panic!("file type for {} does not exist", name))
                                .is_dir()
                                && read_dir(d.path())
                                    .expect("subdirectory not found")
                                    .any(|f| {
                                        f.as_ref().expect("subdirectory file not found").file_name()
                                            == "mod.rs"
                                    })))
                })
                .map(|d| d.file_name().to_string_lossy().into_owned())
                .map(|name| name.trim_end_matches(file_ext).to_owned())
                .filter(|name| name != "main" && name != "mod")
        })
        .flat_map(|v| v.into_iter())
        .collect()
}

fn glob_docs<T: AsRef<str>>(spec: T, file_ext: &str) -> Vec<(String, Vec<(String, String)>)> {
    glob::glob(spec.as_ref())
        .expect("invalid glob pattern")
        .map(|v| v.expect("glob error"))
        .map(PathBufWrapper)
        .map(|v| {
            read_dir(v.0)
                .expect("directory does not exist")
                .map(|d| d.expect("entry does not exist"))
                .filter(|d| {
                    let name = d.file_name();
                    let name = name.to_string_lossy();
                    !name.starts_with('.') && (name.ends_with(file_ext))
                })
                .map(|v| {
                    let mut p = v.path();
                    p.set_extension("");
                    let subs = read_dir(&p)
                        .map(|dir| {
                            dir.map(|v| v.unwrap().file_name().to_string_lossy().into_owned())
                                .map(|n| {
                                    let path = format!(
                                        "{}/{}",
                                        v.file_name().to_string_lossy().trim_end_matches(file_ext),
                                        n
                                    );
                                    (path, n.trim_end_matches(file_ext).to_owned())
                                })
                                .collect()
                        })
                        .unwrap_or_else(|_| {
                            panic!("can't find {}", v.file_name().to_string_lossy())
                        });
                    // .unwrap_or_else(|_| vec![]);
                    let v = v.file_name().to_string_lossy().into_owned();
                    let v = v.trim_end_matches(file_ext).to_owned();
                    (v, subs)
                })
                .filter(|(name, _)| name != "main" && name != "mod")
        })
        .flat_map(|v| v.into_iter())
        .collect()
}

pub fn autoimport<T: AsRef<str>>(input: T) -> TokenStream {
    let module_names = glob_modules(&input, ".rs");
    let module_globs = module_names
        .iter()
        .map(|name| quote::format_ident!("{}", name))
        .collect::<Vec<Ident>>();
    let doc_globs = glob_docs(&input, ".mud");
    let (doc_globs, vecs): (Vec<String>, Vec<TokenStream>) = doc_globs
        .into_iter()
        .map(|(d, sections)| {
            let (s, name): (Vec<String>, Vec<String>) = sections.into_iter().unzip();
            let s = quote! {
                {
                    let mut v = ::std::collections::HashMap::new();
                    #(
                        v.insert(#name.to_owned(), crate::metadata::markdownify(std::include_str!(#s)));
                    )*
                    v
                }
            };
            (d, s)
        })
        .unzip();
    let doc_names = doc_globs.iter().map(|v| format!("{}.mud", v));
    //    assert!(module_globs.len() > 0);
    let mods = module_globs.clone().into_iter();
    let updates = module_globs.clone().into_iter();
    let funcs = module_globs.iter();
    let exports = module_globs.iter();
    let imports = module_globs.iter();
    let modules = module_globs.iter();
    let output = quote! {
        #( mod #mods; )*
        use crate::util::string::Speak;
        pub fn get_migrations() -> ::std::vec::Vec<::std::boxed::Box<dyn ::sea_orm_migration::prelude::MigrationTrait>> {
            let mut v = ::std::vec::Vec::<::std::boxed::Box<dyn ::sea_orm_migration::prelude::MigrationTrait>>::new();
            #(
                if let Some(ref md) = #funcs::METADATA.state {
                    v.append(&mut md.get_migrations());
                }
            )*
            v
        }

        pub async fn all_export(chat: i64) -> crate::util::error::Result<crate::tg::import_export::RoseExport> {
            let mut v = crate::tg::import_export::RoseExport::new();
            #(
                if let Some(ref md) = #exports::METADATA.state {
                    if let (Some(export), Some(name)) = (md.export(chat).await?, md.supports_export()) {
                        v.data.insert(name.to_owned(), export);
                    }
                }
            )*
            Ok(v)
        }

        pub async fn all_import(chat: i64, json: &str) -> crate::util::error::Result<crate::tg::import_export::RoseExport> {
            let mut v: crate::tg::import_export::RoseExport = ::serde_json::from_str(json)?;
            #(
                if let Some(ref md) = #imports::METADATA.state {
                    if let Some(name) = md.supports_export() {
                        if let Some(value) = v.data.remove(name) {
                            md.import(chat, value).await?;
                        }
                    }
                }
            )*
            Ok(v)
        }

        pub fn get_metadata() -> ::std::vec::Vec<crate::metadata::Metadata> {
            let mut metadata = Vec::new();
            #(
                if crate::statics::module_enabled(#module_names) {
                 metadata.push((*#modules::METADATA).clone());
                }
            )*

            #(
                if crate::statics::module_enabled(#module_names) {
                metadata.push(crate::metadata::Metadata {
                        name: #doc_globs.to_owned(),
                        description: crate::metadata::markdownify(std::include_str!(#doc_names)),
                        commands: ::std::collections::HashMap::new(),
                        sections: #vecs,
                        state: None
                    });
                }
            )*

            metadata
        }

        pub(crate) async fn process_updates(
            update: ::botapi::gen_types::UpdateExt,
            helps: ::std::sync::Arc<crate::tg::client::MetadataCollection>,
            handler: crate::tg::client::UpdateHandler
            ) -> crate::util::error::Result<()> {
            match crate::tg::command::StaticContext::get_context(update).await.map(|v| v.yoke()) {
                Ok(ctx) => {
                    if let Err(err) = ctx.record_chat_member().await {
                        log::warn!("failed to record chat member {}", err);
                        err.record_stats();
                    }

                    ctx.handle_gbans().await;

                    if let Err(err) = ctx.greeter_handle_update().await {
                        log::warn!("Failed to greet user {}", err);
                        err.record_stats();
                    }

                    if let Err(err) = ctx.handle_pending_action_update().await {
                        log::warn!("failed to handle pending action: {}", err);
                        err.record_stats();
                    }

                    let help = if let Some(&crate::tg::command::Cmd{cmd, ref args, message, lang, ..}) = ctx.cmd() {
                         match cmd {
                            "help" => crate::tg::client::show_help(&ctx, message, helps, args.args.first().map(|a| a.get_text())).await,
                            "start" => match args.args.first().map(|a| a.get_text()) {
                                Some(v) => {
                                    if let (Some("help"), Some(s)) = (v.get(0..4), v.get(4..)) {
                                        let s = if s.len() > 0 {
                                            Some(s)
                                        } else {
                                            None
                                        };
                                        crate::tg::client::show_help(&ctx, message, helps, s).await?;
                                        Ok(true)
                                    } else {
                                        Ok(false)
                                    }
                                }

                                None => {
                                    log::info!("start with lang {:?}", lang);
                                    message.reply(macros::lang_fmt!(lang, "startcmd")).await?;
                                    Ok(true)
                                }
                            },
                            _ => Ok(false),
                        }
                    } else {
                       Ok(false)
                    };
                    match help {
                        Ok(false) => {
                            handler.handle_update(&ctx).await;
                            #(
                            if crate::statics::module_enabled(#module_names) {
                                if let Err(err) = #updates::update_handler::handle_update(&ctx).await {
                                    err.record_stats();
                                    match err.get_message().await {
                                        Err(err) => {
                                            log::warn!("failed to send error message: {}, what the FLOOP", err);
                                            err.record_stats();
                                        }
                                        Ok(v) => if ! v {
                                            // if let Some(chat) = ctx.chat() {
                                            //     if let Err(err) = chat.reply(err.to_string()).await {
                                            //         log::warn!("triple fault! {}", err);
                                            //     }
                                            // }

                                            log::warn!("handle_update {} error: {}", #updates::METADATA.name, err);
                                        }
                                    }
                                }
                            }
                        )*}
                       Ok(true) => (),
                      Err(err)  => log::warn!("failed help {}", err)
                    }

                }
                Err(err) => {
                    log::warn!("error when getting context {}", err);
                    err.record_stats()
                },
            }
            Ok(())
        }
    };
    output
}
