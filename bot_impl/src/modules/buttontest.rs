use crate::statics::TG;
use crate::tg::command::{Cmd, Context};
use crate::util::error::Result;
use crate::util::string::Speak;
use crate::{metadata::metadata, tg::markdown::MarkupBuilder};
use botapi::gen_types::Message;
use sea_orm_migration::MigrationTrait;

metadata!("Antipiracy",
    r#"
    This is just a debugging module, it will be removed eventually. 
    "#,
    { command = "report", help = "Report a pirate for termination" },
    { command = "crash", help = "Intentionally trigger a floodwait for debugging"},
    { command = "markdown", help = "Reply to a message to parse as markdown"},
    { command = "murkdown", help = "Reply to a message to parse as murkdown" }
);

const BIG: &str = r#"
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Donec sed pretium nibh. Phasellus feugiat nulla sed consectetur porta. Donec ex est, bibendum id purus vitae, faucibus iaculis leo. Ut ut ipsum sem. Sed ac nisl tellus. Donec porttitor nibh ligula, quis euismod tellus dignissim sed. Curabitur non tristique justo.

In hac habitasse platea dictumst. Nam quis suscipit velit. Pellentesque mollis quam eget enim dapibus, mollis hendrerit magna tincidunt. Donec sed hendrerit nisi, eu molestie nunc. Nulla commodo nisi volutpat dui efficitur hendrerit. Suspendisse semper ex turpis, id cursus odio molestie id. In hac habitasse platea dictumst.

Proin efficitur, erat ac rhoncus sodales, lacus justo placerat elit, et bibendum est urna ut quam. Nullam eu sem ut justo porta viverra at in velit. Nunc malesuada sed ante eget rutrum. Sed ac turpis id nibh pharetra sodales id at erat. Nam tellus libero, dictum id elit sit amet, bibendum pellentesque lectus. In sollicitudin, libero vitae vehicula ornare, magna metus rutrum arcu, sed hendrerit urna quam non quam. Donec mollis magna arcu, in consequat ante convallis in. Vestibulum consectetur dui non tellus interdum imperdiet. Donec non sapien ultrices, ornare libero nec, elementum lorem.

Nulla in risus vel mi rhoncus feugiat ut eu odio. Morbi vehicula bibendum justo, eget sagittis neque rutrum quis. Nam efficitur tortor vel mi tincidunt molestie. Aenean arcu lorem, egestas non justo a, blandit molestie tellus. Vestibulum non ligula sed risus ultrices pulvinar quis et ipsum. Nullam facilisis, felis ac varius gravida, risus magna pretium urna, in ultrices dui tellus eu nunc. Vestibulum faucibus egestas dolor ac scelerisque. Nullam urna odio, scelerisque in eleifend sit amet, pretium sed nisi. Sed at nibh id augue placerat euismod et aliquam sapien. Morbi commodo nunc mi. Ut at lacus ut nisi tincidunt placerat sit amet id ante. Etiam varius libero sed sapien tincidunt venenatis. Suspendisse id congue mi. Vestibulum ut nibh accumsan, scelerisque sapien sed, laoreet sem. In ut porttitor orci. Vestibulum sit amet dignissim tortor, eget venenatis nibh.

Ut vitae dolor semper, rutrum sapien vel, gravida turpis. Proin vulputate, mauris id viverra vestibulum, odio tellus rutrum nisl, nec imperdiet ex mauris vitae leo. Praesent consequat, lacus nec efficitur pretium, turpis ipsum vestibulum dui, et condimentum mauris sem vel velit. Donec vulputate eu velit non viverra. Phasellus ac pellentesque nunc, vitae tempor neque. Curabitur auctor tellus massa, ac elementum est suscipit cursus. Integer maximus sit amet lectus sed consequat.

Donec placerat suscipit ex vel pretium. Aenean accumsan erat iaculis suscipit imperdiet. Integer tristique quis orci sed mattis. Proin odio est, vulputate sit amet lectus vitae, placerat tempor odio. Cras accumsan dignissim nibh at ultrices. Nulla blandit faucibus arcu, non volutpat lectus lobortis sit amet. Pellentesque lectus lacus, interdum quis suscipit id, fringilla vel libero. Vivamus eget sapien eget lectus congue mollis eu eget justo. Morbi tincidunt tortor in massa venenatis, et finibus mi auctor.

Proin molestie ante turpis, non elementum risus ultricies nec. Ut mattis et nunc at lacinia. Integer ac dignissim nunc, vel tempus mi. Orci varius natoque penatibus et magnis dis parturient montes, nascetur ridiculus mus. Curabitur eget odio lectus. In hac habitasse platea dictumst. Orci varius natoque penatibus et magnis dis parturient montes, nascetur ridiculus mus. Aliquam et neque at quam facilisis tempus. Vestibulum nisi nisi, vestibulum non neque a, porttitor efficitur leo. Curabitur lectus ex, venenatis in gravida at, congue nec diam. Nulla non tortor massa.

Suspendisse potenti. Quisque volutpat nunc felis, nec laoreet ex pharetra vitae. Nulla facilisi. Proin sed purus a nibh ultrices pellentesque in et massa. Donec gravida, erat in sodales venenatis, nisl libero pulvinar lacus, nec iaculis mi urna lacinia eros. Praesent a rutrum nunc, quis pulvinar odio. Duis sit amet enim vitae massa dapibus lobortis in eget ante. Quisque ut turpis dictum, lobortis magna sit amet, lacinia massa. Nulla pretium condimentum nulla, vestibulum faucibus massa bibendum vitae. Sed sed imperdiet velit. Duis aliquam ipsum quis arcu lobortis sodales. Fusce blandit tortor tempor tincidunt aliquet. Nam laoreet magna quis nunc consectetur aliquam. Vestibulum rutrum orci vitae odio semper, posuere porttitor leo vulputate. Nullam turpis nunc, porta vitae diam et, vehicula posuere purus. Aenean quis blandit nibh.

Suspendisse sit amet pharetra turpis, eu mollis nunc. Sed volutpat aliquet consectetur. Pellentesque non est eu ante dignissim pellentesque. Maecenas finibus dapibus consectetur. Vivamus lobortis metus nisi, at maximus orci pretium sed. Proin volutpat eros odio, sit amet imperdiet augue hendrerit nec. Aenean ullamcorper venenatis enim sed tincidunt. Pellentesque aliquam tellus sit amet cursus pulvinar. Aenean pharetra augue elit, id aliquet lacus pellentesque sit amet. Etiam quam.
"#;

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![]
}

// async fn handle_murkdown(message: &Message) -> Result<bool> {
//     if let Some(message) = message.get_reply_to_message() {
//         if let Some(text) = message.get_text() {
//             match MarkupBuilder::from_murkdown_chatuser(
//                 text,
//                 message.get_chatuser().as_ref(),
//                 None,
//                 false,
//                 false,
//             )
//             .await
//             {
//                 Ok(md) => {
//                     if !should_ignore_chat(message.get_chat().get_id()).await? {
//                         if should_ignore_chat(message.get_chat().get_id()).await? {
//                             return Ok(false);
//                         }
//                         let (msg, entities) = md.build();

//                         TG.client()
//                             .build_send_message(message.get_chat().get_id(), msg)
//                             .entities(entities)
//                             .build()
//                             .await?;
//                     }
//                 }

//                 Err(err) => {
//                     message.speak(lang_fmt!(Lang::En, "test", err)).await?;
//                 }
//             }
//         }
//     }
//     Ok(false)
// }

async fn handle_markdown(message: &Message) -> Result<bool> {
    if let Some(message) = message.get_reply_to_message() {
        if let Some(text) = message.get_text() {
            let md = MarkupBuilder::from_markdown(text, None);
            let (msg, entities) = md.build();
            TG.client()
                .build_send_message(message.get_chat().get_id(), msg)
                .entities(entities)
                .build()
                .await?;
        }
    }
    Ok(false)
}

pub async fn handle_update(ctx: &Context) -> Result<()> {
    if let Some(&Cmd { cmd, message, .. }) = ctx.cmd() {
        // log::info!("piracy command {}", cmd);
        match cmd {
            //            "crash" => TG.client().close().await?,
            "crash" => {
                message
                    .reply("Eh eh ehhh... You didn't say the magic word!")
                    .await?;
            }
            "markdown" => {
                handle_markdown(message).await?;
            }
            "murkdown" => {
                // handle_murkdown(message).await?;
            }
            "biig" => {
                message.reply(BIG).await?;
            }
            _ => (),
        };
    }

    Ok(())
}
