pub trait ChatRefExt {
    fn to_chat_id(&self) -> i64;
}

pub trait UserIdExt {
    fn to_user_i64(&self) -> i64;
}

// Note, according to api docs we should not be getting channel
// usernames from ChatRef
//
// TODO: verify and handle
impl<R: telegram_bot::ToChatRef> ChatRefExt for R {
    #[inline]
    fn to_chat_id(&self) -> i64 {
        match self.to_chat_ref() {
            telegram_bot::ChatRef::Id(id) => id.into(),
            telegram_bot::ChatRef::ChannelUsername(_) => 0,
        }
    }
}

impl<R: telegram_bot::ToUserId> UserIdExt for R {
    #[inline]
    fn to_user_i64(&self) -> i64 {
        self.to_user_id().into()
    }
}
