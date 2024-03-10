//! Helpers for managing federations, subscribable ban lists

use std::collections::{HashMap, HashSet};

use crate::{
    persist::{
        admin::{fbans, fedadmin, federations, gbans},
        core::{chat_members, dialogs, users},
        redis::{default_cache_query, CachedQueryTrait, RedisCache, RedisStr, ToRedisStr},
    },
    statics::{BAN_GOVERNER, CONFIG, DB, REDIS, TG},
    util::error::{BotError, Fail, Result, SpeakErr},
    util::string::Speak,
};

use botapi::gen_types::{
    Chat, EReplyMarkup, InlineKeyboardButtonBuilder, InlineKeyboardMarkup,
    MaybeInaccessibleMessage, UpdateExt, User,
};

use chrono::Duration;

use macros::{entity_fmt, lang_fmt};
use redis::AsyncCommands;

use sea_orm::{
    sea_query::OnConflict, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    EntityTrait, FromQueryResult, IntoActiveModel, JoinType, ModelTrait, QueryFilter, QuerySelect,
    Statement,
};
use sea_query::{
    Alias, ColumnRef, CommonTableExpression, Expr, Query, QueryStatementBuilder, UnionType,
};

use uuid::Uuid;

use super::{
    admin_helpers::insert_user,
    button::{InlineKeyboardBuilder, OnPush},
    command::Context,
    dialog::{get_user_banned_chats, record_chat_member_banned, reset_banned_chats, upsert_dialog},
    markdown::MarkupType,
    user::{GetUser, Username},
};

#[derive(FromQueryResult)]
pub struct FbanWithChat {
    pub fed_id: Uuid,
    pub subscribed: Option<Uuid>,
    pub owner: i64,
    pub fed_name: String,
    pub chat_id: Option<i64>,
    pub fban_id: Option<Uuid>,
    pub federation: Option<Uuid>,
    pub user: Option<i64>,
    pub user_name: Option<String>,
    pub reason: Option<String>,
}

#[inline(always)]
fn get_fed_key(owner: i64) -> String {
    format!("fed:{}", owner)
}

#[inline(always)]
fn get_fban_key(fban: &Uuid) -> String {
    format!("fban:{}", fban.to_string())
}

#[inline(always)]
fn get_gban_key(user: i64) -> String {
    format!("gban:{}", user)
}

#[inline(always)]
fn get_fed_chat_key(chat: i64) -> String {
    format!("fbcs:{}", chat)
}

#[inline(always)]
fn get_fban_set_key(fed: &Uuid) -> String {
    format!("fbs:{}", fed.to_string())
}

pub async fn get_fban_for_chatmember(user: i64, chat: i64) -> Result<Option<fbans::Model>> {
    let result = federations::Entity::find()
        .inner_join(fbans::Entity)
        .inner_join(dialogs::Entity)
        .filter(dialogs::Column::ChatId.eq(chat))
        .filter(fbans::Column::User.eq(user))
        .column_as(dialogs::Column::ChatId, "chat")
        .into_model::<fbans::Model>()
        .one(*DB)
        .await?;

    Ok(result)
}

pub async fn get_fbans_for_user_with_chats(user: i64) -> Result<Vec<FbanWithChat>> {
    let with = Query::with()
        .recursive(true)
        .cte(
            CommonTableExpression::new()
                .table_name(Alias::new("feds"))
                .columns([
                    federations::Column::FedId,
                    federations::Column::Subscribed,
                    federations::Column::Owner,
                    federations::Column::FedName,
                ])
                .query(
                    Query::select()
                        .columns([
                            federations::Column::FedId.as_column_ref(),
                            federations::Column::Subscribed.as_column_ref(),
                            federations::Column::Owner.as_column_ref(),
                            federations::Column::FedName.as_column_ref(),
                        ])
                        .from(federations::Entity)
                        .union(
                            UnionType::Distinct,
                            Query::select()
                                .columns([
                                    federations::Column::FedId.as_column_ref(),
                                    federations::Column::Subscribed.as_column_ref(),
                                    federations::Column::Owner.as_column_ref(),
                                    federations::Column::FedName.as_column_ref(),
                                ])
                                .from(federations::Entity)
                                .join(
                                    JoinType::InnerJoin,
                                    Alias::new("feds"),
                                    Expr::col((
                                        Alias::new("feds"),
                                        federations::Column::Subscribed,
                                    ))
                                    .equals((Alias::new("feds"), federations::Column::FedId)),
                                )
                                .cond_where(
                                    Expr::col((federations::Entity, federations::Column::Owner))
                                        .eq(user),
                                )
                                .to_owned(),
                        )
                        .to_owned(),
                )
                .to_owned(),
        )
        .to_owned();

    let select = Query::select()
        .column(ColumnRef::Asterisk)
        .from(Alias::new("feds"))
        .join(
            JoinType::LeftJoin,
            dialogs::Entity,
            Expr::col((Alias::new("feds"), federations::Column::FedId))
                .equals((dialogs::Entity, dialogs::Column::Federation)),
        )
        .join(
            JoinType::LeftJoin,
            fbans::Entity,
            Expr::col((Alias::new("feds"), federations::Column::FedId))
                .equals((fbans::Entity, fbans::Column::Federation)),
        )
        .join(
            JoinType::LeftJoin,
            chat_members::Entity,
            Expr::col((chat_members::Entity, chat_members::Column::ChatId))
                .equals((dialogs::Entity, dialogs::Column::ChatId)),
        )
        .to_owned();

    let query = select.with(with).to_owned();

    // let result = fbans::Entity::find()
    //     .inner_join(dialogs::Entity)
    //     .filter(fbans::Column::User.eq(user))
    //     .column_as(dialogs::Column::ChatId, "chat")
    //     .find_also_related(federations::Entity)
    //     .into_model::<FbanWithChat, federations::Model>()
    //     .all(*DB)
    //     .await?;
    let backend = DB.get_database_backend();
    let (query, params) = query.build_any(&*backend.get_query_builder());
    log::info!("{}", query);
    let result = federations::Entity::find()
        .from_raw_sql(Statement::from_sql_and_values(backend, query, params))
        .into_model()
        .all(*DB)
        .await?;
    Ok(result)
}

pub async fn get_fbans_for_user(user: i64) -> Result<Vec<fbans::Model>> {
    let result = federations::Entity::find()
        .inner_join(fbans::Entity)
        .inner_join(dialogs::Entity)
        .filter(fbans::Column::User.eq(user))
        .into_model::<fbans::Model>()
        .all(*DB)
        .await?;

    Ok(result)
}

pub async fn is_user_fbanned(user: i64, chat: i64, reply: i64) -> Result<Option<fbans::Model>> {
    if let Some(fed) = is_fedmember(chat).await? {
        log::info!("chat is member of fed {}", fed);
        let key = get_fban_set_key(&fed);
        for _ in 0..2 {
            let (exists, v, l): (bool, Option<RedisStr>, i64) = REDIS
                .try_pipe(|p| Ok(p.exists(&key).hget(&key, user).hlen(&key)))
                .await?;
            if l == 1 {
                log::info!("fban for user {} is emptyset", user);
                return Ok(None);
            }
            if !exists {
                try_update_fban_cache(user).await.unwrap();
            } else if let Some(v) = v {
                let key = get_fban_key(&v.get::<Uuid>()?);
                let fb: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
                if let Some(fb) = fb {
                    return Ok(fb.get()?);
                }
                log::info!("fban cache empty?");
            } else {
                return Ok(None);
            }
        }
        Err(BotError::speak(
            "retries exceeded for updating fban cache",
            chat,
            Some(reply),
        ))
    } else {
        Ok(None)
    }
}

pub async fn get_feds(user: i64) -> Result<Vec<federations::Model>> {
    let feds = federations::Entity::find()
        .left_join(fedadmin::Entity)
        .filter(
            federations::Column::Owner
                .eq(user)
                .or(fedadmin::Column::User.eq(user)),
        )
        .all(*DB)
        .await?;
    Ok(feds)
}

pub async fn create_federation(ctx: &Context, federation: federations::Model) -> Result<()> {
    let key = get_fed_key(federation.owner);
    match federations::Entity::insert(federation.into_active_model())
        .exec_with_returning(*DB)
        .await
    {
        Err(err) => match err {
            sea_orm::DbErr::Query(err) => {
                log::warn!("create fed err {}", err);
                return ctx.fail(lang_fmt!(ctx, "onlyone"));
            }
            err => return Err(err.into()),
        },
        Ok(v) => {
            REDIS
                .try_pipe(|q| {
                    Ok(q.set(&key, Some(v).to_redis()?)
                        .expire(&key, CONFIG.timing.cache_timeout))
                })
                .await?;
        }
    }
    Ok(())
}

pub async fn subfed(fed: &Uuid, sub: &Uuid) -> Result<federations::Model> {
    let model = federations::Entity::update(federations::ActiveModel {
        fed_id: Set(*fed),
        subscribed: Set(Some(*sub)),
        owner: NotSet,
        fed_name: NotSet,
    })
    .exec(*DB)
    .await?;

    let key = get_fed_key(model.owner);
    REDIS.sq(|q| q.del(&key)).await?;
    try_update_fban_cache(model.owner).await?;
    Ok(model)
}

pub async fn update_fed(owner: i64, newname: String) -> Result<federations::Model> {
    let key = get_fed_key(owner);
    let mut model = federations::Entity::update_many()
        .set(federations::ActiveModel {
            fed_id: NotSet,
            subscribed: NotSet,
            owner: Set(owner),
            fed_name: Set(newname),
        })
        .filter(federations::Column::Owner.eq(owner))
        .exec_with_returning(*DB)
        .await?;

    REDIS.sq(|q| q.del(&key)).await?;
    Ok(model
        .pop()
        .ok_or_else(|| BotError::Generic("no fed".to_owned()))?)
}

pub async fn fban_user(fban: fbans::Model, user: &User) -> Result<()> {
    let key = get_fban_key(&fban.fban_id);
    let setkey = get_fban_set_key(&fban.federation);
    insert_user(user).await?;
    let model = fbans::Entity::insert(fban.into_active_model())
        .on_conflict(
            OnConflict::columns([fbans::Column::Federation, fbans::Column::User])
                .update_columns([fbans::Column::Reason, fbans::Column::UserName])
                .to_owned(),
        )
        .exec_with_returning(*DB)
        .await?;
    model.cache(&key).await?;
    REDIS.sq(|q| q.del(&setkey)).await?; //TODO: less drastic
    Ok(())
}

pub async fn get_fed(user: i64) -> Result<Option<federations::Model>> {
    let key = get_fed_key(user);
    for _ in 0..4 {
        let v: Option<RedisStr> = REDIS.sq(|q| q.get(&key)).await?;
        if let Some(v) = v {
            return Ok(v.get()?);
        }
        try_update_fban_cache(user).await?;
    }
    Ok(None)
}

pub async fn is_fedmember(chat: i64) -> Result<Option<Uuid>> {
    let key = get_fed_chat_key(chat);
    for _ in 0..4 {
        let (exists, member, _): (bool, Option<RedisStr>, bool) = REDIS
            .pipe(|p| {
                p.exists(&key)
                    .hget(&key, chat)
                    .expire(&key, CONFIG.timing.cache_timeout)
            })
            .await?;
        match (exists, member) {
            (false, _) => {
                try_update_fed_cache(chat).await?;
            }
            (true, Some(member)) => {
                return Ok(Some(member.get()?));
            }
            (true, None) => {
                return Ok(None);
            }
        };
    }
    Ok(None)
}

pub async fn gban_user(fban: gbans::Model, metadata: User) -> Result<()> {
    let key = get_gban_key(fban.user);

    let user = insert_user(&metadata).await?;
    let model = gbans::Entity::insert(fban.into_active_model())
        .on_conflict(
            OnConflict::column(gbans::Column::User)
                .update_column(gbans::Column::Reason)
                .to_owned(),
        )
        .exec_with_returning(*DB)
        .await?;
    model.join_single(&key, Some(user)).await?;
    Ok(())
}

async fn get_fbanned_chats(fed: &Uuid, user: i64) -> Result<impl Iterator<Item = i64>> {
    let c = dialogs::Entity::find()
        .inner_join(fbans::Entity)
        .filter(
            fbans::Column::Federation
                .eq(*fed)
                .and(fbans::Column::User.eq(user)),
        )
        .all(*DB)
        .await?;
    Ok(c.into_iter().map(|c| c.chat_id))
}

async fn iter_unfban_user(user: i64, fed: &Uuid) -> Result<()> {
    for chat in get_fbanned_chats(fed, user).await? {
        TG.client
            .build_unban_chat_member(chat, user)
            .only_if_banned(true)
            .build()
            .await?;
        BAN_GOVERNER.until_ready().await;
    }
    reset_banned_chats(user).await?;
    Ok(())
}

pub async fn fstat(user: i64) -> Result<impl Iterator<Item = (fbans::Model, federations::Model)>> {
    let res = fbans::Entity::find()
        .filter(fbans::Column::User.eq(user))
        .find_also_related(federations::Entity)
        .all(*DB)
        .await?;
    Ok(res.into_iter().filter_map(|(v, s)| s.map(|u| (v, u))))
}

async fn iter_unban_user(user: i64) -> Result<()> {
    for chat in get_user_banned_chats(user).await? {
        TG.client
            .build_unban_chat_member(chat, user)
            .only_if_banned(true)
            .build()
            .await?;
        BAN_GOVERNER.until_ready().await;
    }
    reset_banned_chats(user).await?;
    Ok(())
}

pub async fn is_user_gbanned(user: i64) -> Result<Option<(gbans::Model, users::Model)>> {
    let key = get_gban_key(user);
    let out = default_cache_query(
        |_, _| async move {
            let o = gbans::Entity::find_by_id(user)
                .find_also_related(users::Entity)
                .one(*DB)
                .await?;
            Ok(o)
        },
        Duration::try_seconds(CONFIG.timing.cache_timeout).unwrap(),
    )
    .query(&key, &())
    .await?;

    Ok(out.map(|(v, u)| {
        let us = v.user;
        (
            v,
            u.unwrap_or_else(|| users::Model {
                user_id: us,
                username: None,
                first_name: "".to_owned(),
                last_name: None,
                is_bot: false,
            }),
        )
    }))
}

#[inline(always)]
fn get_fedadmin_key(fed: &Uuid) -> String {
    format!("fad:{}", fed.to_string())
}

pub async fn fpromote(fed: Uuid, user: i64) -> Result<()> {
    let key = get_fedadmin_key(&fed);
    fedadmin::Entity::insert(
        fedadmin::Model {
            federation: fed,
            user,
        }
        .into_active_model(),
    )
    .on_conflict(
        OnConflict::columns([fedadmin::Column::User, fedadmin::Column::Federation])
            .update_columns([fedadmin::Column::User, fedadmin::Column::Federation])
            .to_owned(),
    )
    .exec(*DB)
    .await?;
    REDIS.sq(|q| q.del(&key)).await?;
    Ok(())
}

pub async fn refresh_fedadmin_cache(fed: &Uuid) -> Result<()> {
    let admins = fedadmin::Entity::find()
        .filter(fedadmin::Column::Federation.eq(*fed))
        .all(*DB)
        .await?;
    let key = get_fedadmin_key(fed);
    REDIS
        .pipe(|p| {
            p.atomic();
            p.del(&key);
            for admin in admins {
                p.sadd(&key, admin.user);
            }
            p
        })
        .await?;
    Ok(())
}

pub async fn is_fedadmin(user: i64, fed: &Uuid) -> Result<bool> {
    let key = get_fedadmin_key(fed);
    for _ in 0..5 {
        let (exists, admin): (bool, bool) =
            REDIS.pipe(|p| p.exists(&key).sismember(&key, user)).await?;
        if !exists {
            refresh_fedadmin_cache(fed).await?;
        } else {
            return Ok(admin);
        }
    }
    Ok(false)
}

pub async fn join_fed(chat: &Chat, fed: &Uuid) -> Result<()> {
    let key = get_fed_chat_key(chat.get_id());
    let mut model = dialogs::Model::from_chat(chat).await?;
    model.federation = Set(Some(*fed));
    upsert_dialog(*DB, model).await?;

    REDIS.sq(|p| p.del(&key)).await?;
    // try_update_fed_cache(chat.get_id()).await?;
    Ok(())
}

pub async fn try_update_fed_cache(chat: i64) -> Result<()> {
    let feds = dialogs::Entity::find()
        .filter(dialogs::Column::ChatId.eq(chat))
        .find_also_related(federations::Entity)
        .all(*DB)
        .await?;
    log::info!("try_update_fed_cache {}", feds.len());

    REDIS
        .try_pipe(|p| {
            let key = get_fed_chat_key(chat);
            p.hset(&key, true, true);

            for (chat, fed) in feds {
                if let Some(fed) = fed {
                    p.hset(&key, chat.chat_id, fed.fed_id.to_redis()?)
                        .expire(&key, CONFIG.timing.cache_timeout);

                    let key = get_fed_key(fed.owner);
                    p.set(&key, Some(fed).to_redis()?)
                        .expire(&key, CONFIG.timing.cache_timeout);
                }
            }
            Ok(p)
        })
        .await?;
    Ok(())
}

type FbanCache = HashMap<Uuid, (HashSet<(i64, Uuid)>, Option<Uuid>)>;

// fn recursive_fban_cache<'a>(
//     p: &'a mut Pipeline,
//     fban_cache: &'a mut FbanCache,
//     seen: &'a mut HashSet<&'a Uuid>,
// ) -> BoxFuture<'a, Result<()>> {
//     async move {
//         log::info!("updating subscribed {:?}", subscribed);

//         while let Some(s) = sub {
//             if seen.len() == fban_cache.len() {
//                 log::info!("traversed!");
//                 break;
//             }
//             if seen.contains(&s) {
//                 log::warn!("somehow found a subscription cycle for fed {}: {}", fed, s);
//                 sub = prev;
//                 continue;
//             }
//             seen.insert(&s);
//             if let Some((fbans, subscribed)) = fban_cache.get(&s) {
//                 for (user, fban) in fbans {
//                     p.hset(&key, user, fban.to_redis()?);
//                 }
//                 prev = sub;
//                 sub = subscribed.as_ref();
//             } else {
//                 sub = None;
//             }
//         }

//         Ok(())
//     }
//     .boxed()
// }

pub async fn try_update_fban_cache(user: i64) -> Result<()> {
    let fbans = get_fbans_for_user_with_chats(user).await?;

    log::info!("update fban cache {}", fbans.len());
    REDIS
        .try_pipe(|p| {
            p.atomic();

            let mut members = HashMap::<i64, Uuid>::with_capacity(fbans.len());
            let mut fban_cache = FbanCache::new();

            let mut s = HashSet::<i64>::new();
            for FbanWithChat {
                fed_id,
                subscribed,
                owner,
                fed_name,
                chat_id,
                fban_id,
                federation,
                user,
                user_name,
                reason,
            } in fbans.into_iter()
            {
                let federation_model = federations::Model {
                    fed_id,
                    subscribed,
                    owner,
                    fed_name,
                };

                if !fban_cache.contains_key(&federation_model.fed_id) {
                    fban_cache.insert(
                        federation_model.fed_id,
                        (HashSet::new(), federation_model.subscribed),
                    );
                }

                let fed_key = get_fed_key(federation_model.owner);
                p.set(&fed_key, Some(&federation_model).to_redis()?)
                    .expire(&fed_key, CONFIG.timing.cache_timeout);
                if let (Some(fban_id), Some(federation), Some(user), Some(chat_id)) =
                    (fban_id, federation, user, chat_id)
                {
                    members.insert(chat_id, federation_model.fed_id);
                    s.insert(chat_id);

                    let fbans = fbans::Model {
                        fban_id,
                        federation,
                        user,
                        user_name,
                        reason,
                    };
                    let fban_key = get_fban_key(&fbans.fban_id);

                    p.set(&fban_key, fbans.to_redis()?)
                        .expire(&fban_key, CONFIG.timing.cache_timeout);

                    if let Some((cache, _)) = fban_cache.get_mut(&federation_model.fed_id) {
                        cache.insert((user, fbans.fban_id));
                    } else {
                        let mut hash = HashSet::<(i64, Uuid)>::new();
                        hash.insert((user, fbans.fban_id));
                        fban_cache.insert(federation_model.fed_id, (hash, subscribed));
                    }
                }
            }

            for (chat, _) in members.iter() {
                let key = get_fed_chat_key(*chat);
                p.del(&key);
            }

            for (chat, fed) in members {
                let key = get_fed_chat_key(chat);
                p.hset(&key, chat, fed.to_redis()?);
                let key = get_fban_set_key(&fed);
                p.del(&key);
                p.hset(&key, true, true);
            }

            for (fed, (fbans, subscribed)) in fban_cache.iter() {
                log::info!("updating subscribed {:?}", subscribed);
                let key = get_fban_set_key(&fed);
                p.hset(&key, true, true);
                for (user, fban) in fbans {
                    p.hset(&key, user, fban.to_redis()?);
                }
                let mut sub = subscribed.as_ref();
                let mut seen = HashSet::<&Uuid>::new();
                while let Some(s) = sub {
                    if seen.len() == fban_cache.len() {
                        log::info!("traversed!");
                        break;
                    }
                    if seen.contains(&s) {
                        log::warn!("somehow found a subscription cycle for fed {}: {}", fed, s);
                        break;
                    }
                    seen.insert(&s);
                    if let Some((fbans, subscribed)) = fban_cache.get(&s) {
                        for (user, fban) in fbans {
                            p.hset(&key, user, fban.to_redis()?);
                        }
                        sub = subscribed.as_ref();
                    } else {
                        sub = None;
                    }
                }
            }

            Ok(p)
        })
        .await?;

    Ok(())
}

impl Context {
    pub async fn unfban(&self, user: i64, fed: &Uuid) -> Result<()> {
        let v = self.try_get()?;
        let chat = v.chat;
        let key = get_fban_set_key(&fed);
        if let Some(fban) = is_user_fbanned(user, chat.get_id(), self.message()?.message_id).await?
        {
            iter_unfban_user(user, &fban.federation).await?;
            fban.delete(*DB).await?;
            REDIS.sq(|q| q.del(&key)).await?;
            self.speak_fmt(entity_fmt!(self, "unfban", user.mention().await?))
                .await?;
        } else {
            self.speak_fmt(entity_fmt!(self, "notfbanned", user.mention().await?))
                .await?;
        }
        Ok(())
    }

    pub async fn fpromote(&self) -> Result<()> {
        self.action_user(|ctx, user, _| async move {
            let c = self.try_get()?;
            let chat = c.chat.get_id();
            let me = ctx
                .message()?
                .get_from()
                .ok_or_else(|| BotError::Generic("no user".to_owned()))?
                .get_id();
            let fed = get_fed(me)
                .await?
                .ok_or_else(|| self.fail_err(lang_fmt!(self, "nofed")))?;
            let mut builder = InlineKeyboardBuilder::default();

            let confirm = InlineKeyboardButtonBuilder::new("Confirm".to_owned())
                .set_callback_data(Uuid::new_v4().to_string())
                .build();

            let cancel = InlineKeyboardButtonBuilder::new("Cancel".to_owned())
                .set_callback_data(Uuid::new_v4().to_string())
                .build();
            let lang = self.lang().clone();
            confirm.on_push_multi(move |callback| async move {
                if callback.get_from().get_id() != user {
                    TG.client
                        .build_answer_callback_query(&callback.get_id())
                        .show_alert(true)
                        .text(&lang_fmt!(lang, "fpromotenotauth"))
                        .build()
                        .await?;
                    return Ok(false);
                }
                if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
                    TG.client
                        .build_delete_message(chat, message.get_message_id())
                        .build()
                        .await?;
                    match fpromote(fed.fed_id, user).await {
                        Ok(_) => {
                            TG.client
                                .build_answer_callback_query(&callback.get_id())
                                .show_alert(true)
                                .text(&lang_fmt!(lang, "fpromoted"))
                                .build()
                                .await
                        }
                        Err(err) => {
                            TG.client
                                .build_answer_callback_query(&callback.get_id())
                                .show_alert(true)
                                .text(&lang_fmt!(lang, "failfpromote", err))
                                .build()
                                .await
                        }
                    }?;
                }

                Ok(true)
            });

            cancel.on_push_multi(move |callback| async move {
                if callback.get_from().get_id() != me {
                    TG.client
                        .build_answer_callback_query(&callback.get_id())
                        .show_alert(true)
                        .text("You are not the fed owner")
                        .build()
                        .await?;
                    return Ok(false);
                }
                if let Some(MaybeInaccessibleMessage::Message(message)) = callback.get_message() {
                    TG.client
                        .build_edit_message_text("Fpromote has been canceled")
                        .message_id(message.get_message_id())
                        .chat_id(chat)
                        .build()
                        .await?;
                    TG.client
                        .build_edit_message_reply_markup()
                        .reply_markup(&InlineKeyboardMarkup::default())
                        .message_id(message.get_message_id())
                        .chat_id(chat)
                        .build()
                        .await?;
                }
                TG.client
                    .build_answer_callback_query(&callback.get_id())
                    .build()
                    .await?;

                Ok(true)
            });

            builder.button(confirm);
            builder.button(cancel);
            if let Some(user) = user.get_cached_user().await? {
                let name = user.name_humanreadable();
                let mention = MarkupType::TextMention(user).text(&name);
                self.speak_fmt(
                    entity_fmt!(ctx, "fpromote", mention)
                        .reply_markup(EReplyMarkup::InlineKeyboardMarkup(builder.build())),
                )
                .await?;
            }
            Ok(())
        })
        .await?;
        Ok(())
    }

    pub async fn ungban_user(&self, user: i64) -> Result<()> {
        let key = get_gban_key(user);

        let delete = gbans::Entity::delete_by_id(user).exec(*DB).await?;
        if delete.rows_affected > 0 {
            REDIS.sq(|q| q.del(&key)).await?;
            tokio::spawn(async move { iter_unban_user(user).await.log() });

            Ok(())
        } else {
            self.fail("User is not gbanned")
        }
    }

    pub async fn handle_gbans(&self) {
        if let UpdateExt::Message(ref message) = self.update() {
            if message.get_sender_chat().is_none() {
                if let Some(user) = message.get_from() {
                    if let Err(err) = self.single_gban(user.get_id()).await {
                        log::warn!("Failed to gban {}: {}", user.name_humanreadable(), err);
                        err.record_stats();
                    }
                }
            }
        }
    }

    async fn single_gban(&self, user: i64) -> Result<()> {
        let chat = self.try_get()?.chat.get_id();
        if let Some((gban, user)) = is_user_gbanned(user).await? {
            record_chat_member_banned(user.user_id, chat, true).await?;

            TG.client
                .build_ban_chat_member(chat, user.user_id)
                .build()
                .await?;
            record_chat_member_banned(user.user_id, chat, true).await?;
            self.speak(format!(
                "User gbanned for {}!",
                gban.reason.unwrap_or_else(|| "piracy".to_owned())
            ))
            .await?;
        }

        if let Some(model) = is_user_fbanned(user, chat, self.message()?.message_id).await? {
            TG.client
                .build_ban_chat_member(chat, model.user)
                .build()
                .await?;
            record_chat_member_banned(user, chat, true).await?;
            self.speak(format!(
                "User fbanned for {}!",
                model.reason.unwrap_or_else(|| "piracy".to_owned())
            ))
            .await?;
        }
        Ok(())
    }
}
