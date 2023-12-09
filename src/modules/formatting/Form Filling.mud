[*Form filling]  
Murkdown supports filling in information about the current chat and the user interacting with the message.
These items are context dependent and allow messages to be personalized for a specific chat or user.  
Form filling works by surounding a keyword in curly braces \([`\{keyword\}]\).

Available keywords include:  
[_username]: The user's @handle if it exists, the user's first name as a mention if it does not.  
[_first]: The user's first name  
[_last]: The user's last name  
[_mention]: User the user's first name  to ping them  
[_chatname]: The full name of the current chat  
[_id]: The user's id number


