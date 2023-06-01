[*Formatting using murkdown]  

Murkdown is inspired by telegram's own markdown, with minimal modifications to allow parsing via a context-free
grammar.  
This is intended to allow for more reliable detection of valid syntax

[*Key differences from markdown]  
Translating between markdown and murkdown is easy. Just take the relevent markdown character and insert
a single unmatched instance of that character into a pair of square brackets.

So with these rules:  
- \*bold\* would become \[\*bold\]  
- \|\|spoiler\|\| would become \[\|\|spoiler\]

Links in murkdown are the same as links in markdown, \[link name\]\(https://example.com\)

Murkdown special characters include [`\_ \| \~ \` \* \[ \] \( \) \{ \} \< \>]
and must be manually escaped using a backslash \(\\\) like this: \\\*  
If murkdown fails to parse, the message is emitted verbatim without any escaping required.
This means that if you don't want to escape every character and don't need formatting,
just write text! it will get printed no matter what.

[*Full examples]  
\[\`code block\] =\> [`code block]  
\[\*bold text\] =\> [*bold text]  
\[link name\]\(https://ustc.edu.cn\) =\> [link name](https://ustc.edu.cn)  
\[\~strikethrough text\] =\> [~strikethrough text]  
\[\_italic text\] =\> [_italic text]  
\[\_\_underline text\] =\> [__underline text]  
\[\|\|spoiler text\] =\> [||spoiler text]

[*Buttons]  
Murkdown has special syntax for declaring buttons to be attached to messages.
Currently this works on notes, support will be added soon to blocklists and welcome messages.

To add a button to a message use  
[_url button]: \<button text\>\(https://example.com\)  
[_note button]: \<button text\>\(#notename\)

Url buttons are self-explanitory. They contain a link to a web resource. It should be noted
that the url should be valid, or telegram will throw an error code

Note buttons are buttons that link to an existing note \(using the notes module\).
when a user clicks the button they will be redirected to the bot's DM to avoid spamming
the chat and the requested note will be sent.  
Additional note buttons clicked on while in DM will result in editing the message to that note.
This is particularly useful for creating menu-like interfaces.

[*Troubleshooting]  
The most common issue with murkdown is formatting not applying and the message's text being
printed verbatim. This is the default response to a parse error. It is meant to make sending
non-murkdown messages easier. Make sure you have escaped all non-murkdown special characters to
get your message to print with formatting
