### Every sentence the window says to a person, in English: the catalog every other language translates.
### A message without a value is a `Phrase`, which may name a control and so is plain text no longer than a label in
### every catalog, or, where its comment begins `Text:`, a `Text`, shown only as text and as long as a language needs;
### one with values is a method on `Words`. `build.rs` generates them.
### A sentence a server wrote is its own and is not here.

# This catalog's language in its own name, which Settings › Appearance lists it by.
language-name = English
# The title of the window, which a task switcher and a screen reader say.
window-title = Chatkord
saved-sign-in-expired = The saved sign-in has expired; enter your password.
# The server did not answer while a saved sign-in was being offered, so nothing about that
# sign-in is thrown away: the words say the server, not the person, and `TryAgain` is what
# they can do next.
saved-sign-in-held = That server did not answer; your saved sign-in is kept, so nothing was lost.
keyring-unavailable = The system keyring is not available, so you will be asked for your password next time.
# Text: a forget or a sign-out the keyring would not carry out.
token-left = The system keyring could not be used, so this server’s saved sign-in is still on this computer.
username-label = Username
password-label = Password
show-password = Show password
hide-password = Hide password
sign-in-action = Sign in
# Beside an address reached without encryption, in the warning colour; an encrypted one has no word.
route-plain = not encrypted
no-channels-yet = No channels yet.
try-again = Try again
leave-call = Leave
back-to-channels = Back to the channels
joining-call = Joining the call.
# The call button's action while the microphone is on.
mute-mic = Mute
# The same while it is off.
unmute-mic = Unmute
# The call button's action while hearing.
deafen-self = Deafen
# The same while deafened.
undeafen-self = Undeafen
# The call buttons' name for leaving.
leave-the-call = Leave the call
# The arrow beside mute, which opens the lists of microphones and speakers.
device-menu = Microphone and speaker
# The two lists' headings in that menu.
microphone = Microphone
speaker = Speaker
# The first row of each list, which follows the system's choice.
system-default = Default
# Takes the seat in a call back from another device.
switch-here = Switch to this device
# What a screen reader says after this account's own name.
you-are = you
# The side pane's call panel, which opens the call.
open-the-call = Open the call
# Beside this account in a channel it is in on another device.
other-device = other device
# Under the title of a call whose channel was deleted.
deleted-body = You left the call when the channel was deleted.
# A call the window could not keep up with, ended.
fell-behind = The window fell behind, so you left the call.
# The share panel's and Settings' rows: their names, a ceiling's hint, and the choice the rules make.
share-resolution = Resolution
share-frame-rate = Frame rate
share-bitrate = Bitrate
# The share panel's Bitrate row: its heading, its custom rate's slider and field for a screen reader, and the note under
# that field: the ceiling, or why the rate typed is none.
share-bitrate-heading = Bitrate (Mbit/s)
share-rate-label = Most the share sends, in Mbit/s
# The unit after the custom rate's field.
share-rate-unit = Mbit/s
share-rate-at-most = At most { $rate } Mbit/s; less when the screen or your connection needs less.
share-rate-out-of-range = Pick a rate from { $least } to { $most } Mbit/s, in steps of { $step }.
share-codec = Codec
automatic = Automatic
# The share's own size, the first resolution.
source = Source
# The call button that sends this device's screen.
share-screen = Share
# The same while sharing.
stop-sharing = Stop sharing
# Why the share button is dimmed on a device with nothing to encode a screen.
cant-share-no-encoder = This device can’t share: its graphics card has no video encoder Chatkord can use.
# Why it is dimmed for an account whose roles do not allow it.
cant-share-not-allowed = You can’t share on this server.
turn-on-camera = Turn on camera
stop-camera = Stop camera
cant-camera-not-allowed = You can’t use the camera on this server.
# The share panel's and Settings' sound switches, with their hints.
share-screen-sound = Share system sound with a whole screen
share-screen-sound-hint = Never Chatkord’s own.
share-window-sound = Share the app’s sound with a window
# One's own share's popover: what its sound is.
sound-everything = Sound: every app except Chatkord
sound-off = Sound: off
sound-not-found = No sound found for this window
pick-sound-app = Pick the app whose sound to share:
sound-own = No sound: Chatkord’s own is never shared
# The share panel's one action: the desktop's own picker, or ours.
choose-screen = Choose a screen or window…
# Our own picker (Windows): its title, its two tabs and its action.
choose-what-to-share = Choose what to share
pick-screens = Screens
pick-applications = Applications
share-this = Share
# On a shared screen: the size, the rate and how it is drawn.
about-picture = About this picture
# On a card: it fills the stage.
pin-screen = Pin
# On a pinned screen: back to its card.
unpin-screen = Unpin
# On the card of a share or a camera: it stops arriving here until it is resumed.
pause-stream = Pause
# On the card of one paused: it plays again.
resume-stream = Resume
# The badge on a card whose picture was paused.
paused = Paused
# In the popover about one's own stream or camera while nobody watches it, under its size.
not-sending-note = Not sending: no viewers.
# Under a pinned card: the strip of the other cards is put away, or shown again.
hide-strip = Hide people
show-strip = Show people
# On the floating card that keeps a pinned screen playing while its call is out of view.
go-to-call = Go to the call
reset-position = Reset position
# On a watched screen's card: it fills the whole display.
full-screen = Full screen
# Over a full screen: back to the window.
leave-full-screen = Leave full screen
# On a watched screen's card: it plays in a window of its own, kept above the others where the system allows it.
picture-in-picture = Picture in picture
# On the card of a screen in its own window: it plays in its card again.
close-picture-in-picture = Close picture in picture
# Over a full screen, at its top, until the pointer rests.
press-esc-to-leave = Press Esc to leave full screen
# On a watched screen shown large: the wheel zooms it and a drag moves it, or it goes back to the whole picture.
zoom-picture = Zoom
stop-zooming = Stop zooming
# The name on this account's own screen's card.
your-screen = Your stream
# On that card while its first picture is awaited.
share-starting = Starting your share…
# On that card, where this device cannot show what it sends.
own-screen-unshown = Your stream can’t be shown on this device.
# A warning in the call: the share's encoder failed and another took over.
share-now-on-processor = Your stream’s encoder stopped; the processor encodes it now.
share-now-on-another-encoder = Your stream’s encoder stopped; another one encodes it now.
# A warning in the call: the decoder of a screen this viewer watches failed and another took over.
watched-now-on-processor = The decoder of { $name }’s stream stopped; the processor decodes it now.
watched-now-on-another-decoder = The decoder of { $name }’s stream stopped; another one decodes it now.
# In that popover: how the picture is decoded, with the interface after it, or that it fell back to software.
hardware-decoded = Hardware decoded
software-decoded = Software decoded
# In the popover about this account's own screen, with the interface after it, and under the share panel's codec row:
# how the screen is encoded.
hardware-encoded = Hardware encoded
software-encoded = Software encoded
# After a hardware path whose encoder nobody has tested with Chatkord (NVENC, Quick Sync): "Hardware encoded, untested".
untested = untested
# The same popover while the connection keeps the encoder below what the picture is allowed for a few seconds.
held-back = Held back by your connection
# The action of a notice that a share stopped.
share-again = Share again
# The notice when the server ended a share because the account's roles no longer allow one.
share-denied = You can’t share on this server any more, so your share stopped.
camera-not-allowed = You can’t use your camera on this server any more, so it was turned off.
# The heading of the share panel.
share-to-call = Share
# On a shared screen's card while its first picture is awaited.
share-waiting = Waiting for the stream.
# The card's word for pictures this viewer did not paint in time.
share-dropped = pictures not painted
# The same, for one.
one-unpainted = 1 picture not painted
presence-online = Online
presence-speaking = Speaking
presence-muted = Muted
presence-deafened = Deafened
presence-muted-by-admin = Muted by an admin
mute-for-everyone = Mute for everyone
undo-mute-for-everyone = Undo mute for everyone
take-out-of-voice = Take out of voice
volume = Volume
# A sound silenced on this device alone, beside its volume, and its undo.
mute-for-me = Mute for me
unmute-for-me = Unmute for me
# The call header's button and the panel it opens: every sound in the call but one's own.
audio-controls = Audio controls
audio-controls-empty = No one else is in the call.
as-an-admin = As an admin
join-again = Join again
presence-connecting = Connecting
presence-failed = Failed
presence-live = Live
presence-reconnecting = Reconnecting
# The side pane's notice of a new version of Chatkord, which About Chatkord says too while it downloads.
update-downloading = Downloading the new version.
update-ready = The new version is ready. Restart to use it.
# The desktop popup that offers the debug report after a run that did not end cleanly.
earlier-run-ended-title = Chatkord did not close properly last time
earlier-run-ended-body = Click to save a debug report with the log of that run.
update-failed = The update could not be downloaded. It will be tried again later.
update-waits-for-call = The new version is ready. It waits until you leave the call.
# The notice's and About Chatkord's button that starts the new version.
update-restart = Restart
add-title = Add a server
add-body = Paste the invite link you were sent, or type the address of a server you already use.
where-label = Invite link or server address
# The empty field's example: a server address.
where-placeholder = chat.example.org
paste = Paste
continue = Continue
preview-title = Is this the right server?
not-this-one = Not this one? Change
invite-works = ✓ Your invite works
owner-invite = Owner invite
sign-in-there = You’ll sign in with your account there.
# Text: an invite the server refused, under the address.
invite-refused = This invite doesn’t work any more. It was used up or ran out of time. Ask whoever sent it for a new one.
back = Back
have-account = I have an account there
name-hint = Letters, numbers, dots, dashes and underscores. Capitals are kept.
name-characters = Use letters, numbers, dots, dashes or underscores.
sign-in-instead = Sign in instead
password-again-label = Password again
long-enough = ✓ Long enough
matches = ✓ Match
differs = Doesn’t match
too-common = Too common to be safe
create-action = Create account and join
forgot-password = Forgot password?
ok = OK
new-here = New here?
create-an-account = Create an account
need-invite = New here? You’ll need an invite.
paste-one = Paste one
cancel = Cancel
new-password-title = Set a new password
new-password-label = New password
new-password-again-label = New password again
save-and-continue = Save and continue
saving-password = Saving your password…
# The name beside the logo on the welcome window.
wordmark = chatkord
tagline = Voice, screen sharing and chat for your community.
later = Later
quiet-for-now = No one else is here yet. Invite people to get started.
# The same note once others have joined.
invite-more = Invite more people to join you here.
too-many-tries = Too many tries. Wait a few minutes, then try again.
# A certificate expired, revoked or for another name, or a server once vouched for that now signs its own.
untrusted = This server’s certificate is not valid, so Chatkord will not connect to it.
# A server that speaks only TLS 1.2 or older.
old-encryption = This server uses an outdated version of TLS, so Chatkord will not connect to it.
# The first-use prompt for a server whose certificate no authority signed: what it is, then what to weigh.
self-signed = This server uses a self-signed certificate.
# Text: what to weigh before trusting it.
self-signed-body = Chatkord cannot verify who operates it. Continue only if you know and trust the server’s administrator.
# Over the SHA-256 of a server's key, shown as information.
fingerprint = Fingerprint
trust-server = Trust this server
# A server that presented another key than the one this device trusts for it, and what that may mean.
key-changed = The certificate this server presented does not match the one this device trusts.
key-changed-why = The server’s administrator may have replaced it, or the connection may be intercepted.
# Text: what to do about it.
key-changed-body = Nothing was sent to the server. Trust the new certificate only if its administrator confirms the change.
previous-fingerprint = Previous fingerprint
new-fingerprint = New fingerprint
trust-new-certificate = Trust the new certificate
other-version = That server runs another version of Chatkord.
# An ask the server does not know because it runs an older version.
server-too-old = This server is older than this app.
# The server speaks a newer major than this app.
update-to-join = Update Chatkord to join this server.
# Something pasted as an invite link that is not one: cut short, or with more after it.
not-a-link = That isn’t a whole invite link. Paste the link you were sent.
# The content area of a server with no text channel, while no call is open.
pick-voice-channel = Pick a voice channel to join.
# The same where the server has a text channel.
pick-channel = Pick a channel to read or join.
# While the pane is folded into a drawer: the content header's button and the empty page's, which open it.
show-channels-button = Channels
show-channels = Show channels
# The grip on the edge of the server pane that resizes it.
pane-width = Channels width
# On a settings page shown alone in a narrow window: back to the list of pages.
settings-back = Back
pane-not-answering = Not answering right now.
pane-connecting = Connecting…
pane-signed-out = Not signed in on this device.
# The pane of a server whose key changed: no connection is made to it.
pane-not-connected = Not connected.
# The account strip of a server not signed in.
not-signed-in = Not signed in
# The side pane's heading over the voice channels.
voice-section = Voice
try-now = Try now
pick-another = Pick another server in the list at any time.
dismiss = Dismiss
# After a server's name in the rail, for a screen reader: what its dot or ring says.
rail-not-answering = not answering
rail-signed-out = signed out
rail-connecting = connecting
rail-identity-changed = identity changed
# After a server's name in the rail: the call this window is in is on it.
rail-in-call = you’re in a call here
# Beside the username a signed-out server remembers: sign in as someone else.
not-you = Not you? Use another account
# A sign-in under way on a server's own panel.
signing-in-now = Signing in…
close = Close
copy-address = Copy server address
remove-here = Remove from this device
sign-out-action = Sign out
sign-out-body = You’ll need your password to sign in again on this device.
quit-title = Quit Chatkord?
quit-action = Quit
# Text: the note in the confirmation to remove a server.
remove-body = You’ll be signed out here. Your account and the server stay. To come back, you’ll need its address or an invite.
owner-leave-body = Give ownership to another member before you leave. Removing it from this device is always possible.
your-password = Your password
password-doesnt-match = That password doesn’t match.
sign-out-instead = Sign out instead
invite-people = Invite people
server-settings = Server settings
can-be-used-by = Can be used by
works-for = Works for
note-label = Note
# The invite dialog's Role field, and the invites table's column of it.
role-label = Role
# What an invite with no other role gives, said where the server's roles are not read yet.
member-role = Member
# A segment of a number of one's own: an invite's people, a share's bitrate.
custom = Custom
# What the custom number's slider and field are named for a screen reader.
uses-label = People who can use it
# An invite that gives a role this account may not give.
invite-role-refused = You can’t give that role.
optional = Optional
who-is-it-for = Who is it for?
create-invite-link = Create invite link
send-this-link = Send this link to the people you are inviting.
invite-link-label = Invite link
copy = Copy
copied-mark = ✓ Copied
manage-invites = Manage invites
done = Done
invites = Invites
new-invite = New invite
used-head = Used
ends-head = Ends
no-invites = No invites working right now. Make one to invite people.
no-note = No note
delete-invite-title = Delete this invite?
delete-invite-body = Anyone who hasn’t used it yet won’t be able to join with it.
keep-it = Keep it
delete-invite = Delete invite
cant-delete-invite = You can’t delete this invite.
plain-text-only = Use plain text only.
members = Members
reset-password-action = Reset password
make-owner = Make owner
choose-new-owner = Choose a new owner
# Server settings › Members: its two lists, the search over them, and a banned member's row.
who-to-list = Who to list
banned = Banned
search-members = Search members
search-by-name = Search by name or username
clear-search = Clear search
no-one-banned = No one is banned.
unban = Unban
# Server settings › Roles: its heading, main action and what it says of the order.
roles = Roles
create-role = Create role
roles-hint = Members act only on the members and roles below their highest role.
# Its second sentence, said after it in the same paragraph.
roles-hint-move = Drag a role, or press Alt and an arrow key, to change its place.
# The list of roles, as a screen reader names it.
roles-highest-first = Roles, highest first
# Beside the owner role in the list, and beside the default role.
server-owner = Server owner
everyone = Everyone
role-name = Role name
role-colour-help = Shown beside the members who hold it.
# Text: in place of the owner role's settings.
owner-role-body = The owner holds every word and acts on everyone. This role moves only when the server’s ownership does, from Members.
default-role-body = Everyone holds it, and it stays last.
give-only-held = You can give only the words you hold.
delete-role = Delete role
create-role-title = Create a role
# The new role's name field, while empty.
new-role = New role
# Text: the note in the confirmation to delete a role.
delete-role-body = Everyone who holds it loses it, and an invite that gives it stops working. This can’t be undone.
# The groups of a role's grant words.
words-channels = Channels and messages
words-calls = Calls
words-server = The server
# Each grant word: its name, and the line under it.
grant-channel-read = See channels
grant-channel-read-line = See a channel, read its messages and hear its calls.
grant-send-message = Send messages
grant-send-message-line = Write in text channels, and edit and delete their own.
grant-add-reactions = Add reactions
grant-add-reactions-line = React to messages with emoji.
grant-attach-files = Attach files
grant-attach-files-line = Send files and pictures in text channels.
grant-manage-messages = Delete messages
grant-manage-messages-line = Delete anyone’s message, not only their own.
grant-pin-messages = Pin messages
grant-pin-messages-line = Pin and unpin messages in text channels.
grant-mention-everyone = Mention everyone
grant-mention-everyone-line = Use @everyone, @here and any role’s mention.
grant-start-voice = Join calls
grant-start-voice-line = Join voice channels and speak.
grant-share-screen = Share screen
grant-share-screen-line = Share a screen and its sound in a call.
grant-share-camera = Turn on camera
grant-share-camera-line = Send their camera in a call.
grant-moderate-voice = Moderate calls
grant-moderate-voice-line = Mute someone for everyone, or take them out of a call.
grant-invite-member = Invite people
grant-invite-member-line = Make, see and delete invite links.
grant-manage-channel = Manage channels
grant-manage-channel-line = Make, rename, move and delete channels.
grant-manage-members = Manage members
grant-manage-members-line = See usernames; kick, ban and unban; reset a password.
grant-manage-roles = Manage roles
grant-manage-roles-line = Make and change the roles below their own, and give them.
grant-manage-emoji = Manage emoji
grant-manage-emoji-line = Upload, rename and delete the server’s own emoji.
grant-manage-webhooks = Manage webhooks
grant-manage-webhooks-line = Make, regenerate and delete a channel’s webhooks.
grant-manage-server = Manage server
grant-manage-server-line = Change the server’s name, colour and picture.
channels = Channels
create-channel = Create channel
create-channel-title = Create a channel
create-voice-channel = Create a voice channel
channel-name-label = Name
channel-example = Movie night
rename = Rename
delete = Delete
save = Save
delete-voice-body = Everyone in it leaves the call. This can’t be undone.
cant-manage-channels = You can’t manage channels.
channel-gone = This channel was deleted.
# The person's profile page, in its nav and over it.
# Beside the username on the profile page.
cant-be-changed = Can’t be changed
name-others-see = Name others see
profile-picture = Profile picture
# Beside the name others see: a change not saved yet.
press-enter-to-save = Press Enter to save
saving = Saving…
saved = ✓ Saved
name-not-accepted = That name can’t be used. Choose another.
# The name of the person's own page of settings, in its nav and over it.
account = Account
change-password-action = Change password
change-password-title = Change password
change-password-line = Changing it signs you out on your other devices.
sign-in-heading = Sign-in
current-password-label = Current password
save-password-action = Save password
same-as-now = That’s the password you have now.
password-not-accepted = That password can’t be used. Choose another.
on-this-device = On this device.
leave-action = Leave
# A toast.
password-changed = Password changed. Your other devices are signed out.
# The gear on the account strip, and the title over the settings pages.
settings-label = Settings
# The connection popover: its title (also the bars' name), the three grades, its two columns and their rows.
stats-title = Connection
quality-good = Good
quality-fair = Fair
quality-poor = Poor
stats-sending = Sending
stats-receiving = Receiving
stats-rate = Rate
stats-packets = Packets
stats-round-trip = Round trip
# Under the connection popover's graph, at its latest look.
stats-now = now
stats-lost = Lost
stats-this-call = This call
stats-alone = No one else is sending.
# The stats popover's section about this account's own share.
stats-your-share = Your stream
# The stats popover's section about this account's own camera, its rows the share's.
stats-your-camera = Your camera
# The name a screen reader says of that popover's tabs for one's own stream and camera, while both are sent.
stats-your-video = Your video
# A row of that section.
stats-codec = Codec
# A row of that section.
stats-encoder = Encoder
# A row of that section.
stats-size = Size
# A row of that section.
stats-frame-rate = Frame rate
# That row's figure while nobody watches the stream, so it sends none.
stats-not-sending = Not sending
# The rate row's figure while nobody watches the stream.
stats-rate-none = —
# A row of that section.
stats-frames-sent = Frames sent
stats-key-frames = Key frames
stats-resent = Resent
stats-restarts = Encoder restarts
# A row of that section, shown only while the link holds the picture back.
stats-limited-by = Limited by
# What limits it.
stats-bandwidth = bandwidth
# The encoder's path, with the interface after it.
hardware = Hardware
software = Software
# Settings › Voice and video: the page's name, the nav group it is in, what the microphone's bar says, the test
# sound's button and its toast, and a saved device that is not plugged in.
voice-and-video = Voice and video
notifications = Notifications
# A call's Voice options, beside Audio controls: how the call is drawn.
voice-options = Voice options
hide-people = Hide people with no picture
always-show-controls = Always show voice controls
hide-own-share = Hide my own share
# Settings › Appearance and what it holds.
appearance = Appearance
theme-label = Theme
theme-system = Match system
theme-light = Light
theme-dark = Dark
colour-label = Colour
colour-area = Colour and brightness
colour-hue = Hue
colour-hex = Hex
colour-hex-hint = Use six hex digits, like #FF8A5B.
overview = Overview
# The server's picture row: its buttons, and the word while one goes up.
choose-picture = Choose a picture
change-picture = Change picture
remove-picture = Remove
uploading = Uploading…
server-name = Server name
# The profile's colour: its name, and under it who sees it, or that none is picked yet.
your-colour = Your colour
name-not-allowed = Only an admin can change this.
set-up-body = You own this server. Give it a name, a picture and a colour people will recognise.
skip-for-now = Skip for now
more-contrast = More contrast
# Settings › Appearance: the language the window is said in, and its first choice, the system's.
language-label = Language
language-system = Match system
# The Language field while it matches the system: the language the system asks for, in its own name.
language-system-named = Match system ({ $system })
zoom-label = Zoom
zoom-in = Zoom in
zoom-out = Zoom out
zoom-reset = Reset
zoom-keys = Ctrl + and Ctrl − zoom in and out. Ctrl 0 resets.
# Settings › Appearance: how a text channel's messages are drawn, each choice in pixels with its default, and a
# preview of two runs drawn as chosen.
messages-heading = Messages
message-size-label = Text size
message-size-note = In pixels; { $default } is the default. Zoom sizes the whole window.
run-gap-label = Space between message groups
run-gap-note = In pixels; { $default } is the default.
preview-label = Preview
# The preview's two runs: a question in two messages and an answer, each with its author's name.
preview-first-author = grace
preview-first-line = who’s around tonight?
preview-second-line = anyone up for a game later?
preview-reply-author = Bo
preview-reply-line = yes! give me ten minutes
this-device = This device
test-microphone = Speak to test your microphone
play-test-sound = Play a test sound
# The microphone test: one's voice played back a little late, and what is said while it plays.
test-voice = Test
stop-test = Stop test
testing-voice = You hear yourself a moment late. Use headphones.
# The voice settings' camera card: each camera, the picture asked of it, a preview and the mirror.
camera = Camera
camera-resolution = Resolution
camera-frame-rate = Frame rate
test-camera = Test camera
starting-camera = Starting camera…
mirror-camera = Mirror my camera
# What a screen reader says of a person in the side pane's list whose camera is on.
camera-on = Camera on
# A loudness or a gain, in whole decibels: "-48 dB".
decibels = { $db } dB
# The voice settings' card of what the microphone goes through before it is sent.
voice-processing = Voice processing
echo-cancellation = Echo cancellation
echo-cancellation-hint = Keeps what your speakers play out of your microphone.
noise-suppression = Noise suppression
suppression-off = Off
suppression-standard = Standard
suppression-rnnoise = RNNoise
suppression-rnnoise-hint = Removes more background noise, like typing and fans.
automatic-gain = Automatic gain
automatic-gain-hint = Evens out how loud you sound.
microphone-volume = Microphone volume
microphone-volume-hint = Turn off automatic gain to set it yourself.
noise-gate = Noise gate
noise-gate-hint = Sends nothing while you are quiet.
gate-threshold = Threshold
set-from-room = Set from my room
# The voice settings' switch of the window's own sounds, and what it covers.
event-sounds = Event sounds
# Settings › Notifications: the sound of a message that would alert, kept apart from the window's own sounds.
message-sounds = Message sounds
message-sounds-note = A sound when a message would alert you, unless you’re reading its conversation.
# Settings › Startup and window: whether Chatkord opens at sign-in, where it opens then, and what closing does.
startup-and-window = Startup and window
at-sign-in = Open Chatkord when you sign in to this device
start-minimized = Start in the system tray
close-to-tray = Closing the window keeps Chatkord running
close-to-tray-hint = Quit from the tray icon.
no-tray = Some Linux desktops show no tray. There, closing the window quits Chatkord.
# The About page of this device's settings and of a server's: what the build is, its licence and where its source is,
# and the projects it is built with.
about = About
about-version = Version
about-protocol = Protocol
about-licence = Licence
about-source = Source
about-client = Client
about-servers = Servers
about-server = Server
# Where this build's source is: a release's tag ("tag v0.9.0"), the commit a pre-release was built from, or neither
# for a build run from its own tree.
source-tag = tag { $tag }
source-commit = commit { $commit }
source-local = a local build
# Text: the licence, under the client's facts.
free-software = Chatkord is free software: you may share and change it under the GNU General Public License, version 3 or later.
built-with = Built with
built-with-body = These projects are part of Chatkord, each under its own licence.
notices-in-package = The notices come with the installed package.
# About Chatkord's debug report: the row, its dialog and the file it saves. The report holds only counts, words and
# product names, never a name, an address or a message.
debug-report = Debug report
client-debug-report = Client debug report
client-debug-report-line = Contains no names, addresses or messages.
download-report = Download
# At the foot of the first-run window, where a person who cannot get further can still make the client's report.
report-ask = Something not working?
report-ask-link = Download a debug report
server-debug-report = Server debug report
server-debug-report-line = Contains no member data, messages, addresses or keys.
# Text: the note in the debug report's dialog.
report-body = The file contains only what is listed below; a long log is listed by its newest lines. It is not sent anywhere.
report-downloaded = Downloaded as { $file }.
report-not-saved = Couldn't save the file.
report-not-made = Couldn't make the report.
report-preparing = Preparing…
report-too-many = Too many requests. Try again shortly.
report-gone = The report is no longer there. Ask for it again.
# The report's dialog lists a log's newest lines: how many are shown, of how many the log holds.
report-log-cut = Showing the last { $shown } of { $total } lines; the file holds them all.
# About Chatkord's Updates: where this build's update stands, and the switch that has Chatkord check by itself.
updates = Updates
update-unchecked = Chatkord has not checked for updates yet.
update-checking = Checking for updates…
update-up-to-date = Chatkord is up to date.
update-version-ready = Version { $version } is ready. Restart to use it.
update-version-waits = Version { $version } is ready. It waits until you leave the call.
update-not-checked = Couldn't check for updates. Try again later.
updates-in-package = Updates come with the installed package.
check-for-updates = Check for updates
auto-update = Download updates automatically
auto-update-hint = Chatkord checks when it starts and every few hours.
# The tray icon's menu; its Mute and Deafen are the call bar's words.
tray-open = Open Chatkord
tray-quit = Quit Chatkord
tray-restart = Restart to update
not-connected = Not connected
nothing-answers = Nothing answers at that address.
no-such-host = No server could be found by that name.
no-network = This device isn’t connected to the internet. Check your connection, then try again.
not-a-chatkord-server = That address is not a Chatkord server.
no-answer-in-time = The server did not answer in time.
server-stopped-answering = The server stopped answering.
connection-ended = The connection to the server ended.
not-connected-there = This device is not connected to that server now.
no-greeting = That server took the connection but did not answer in time.
start-no-graphics = Chatkord cannot start: no graphics card here can show its window.
start-no-window = Chatkord cannot start: its window could not be opened.
start-failed = Chatkord cannot start. Something went wrong that it could not get past.
start-too-slow = Chatkord took too long to start, so it stopped. Its log says where it waited.
server-unreadable = That server answered something Chatkord can’t read.
# A refusal, by the code the server answered with.
refused-unsaid = The server refused without saying why.
refused-unknown-op = That server doesn’t know this request.
refused-unreadable = The server couldn’t read what this device sent.
refused-signed-out = This session is not signed in; sign in again.
refused-not-allowed = Your account may not do that.
refused-too-soon = That was too soon; try again in a moment.
refused-unavailable = That server isn’t available now; try again later.
refused-not-found = That isn’t there, or your account can’t see it.
refused-invalid = The server couldn’t take that request.
refused-already-in-voice = This account is already in voice.
refused-name-taken = That name is taken.
refused-store-full = The server’s disk is full.
refused-list-changed = The list changed meanwhile; try again.
refused-no-voice-address = This server can’t take calls until its public address is set.
refused-no-voice-address-manage = Set this server’s public address in Server settings › Hosting so calls can connect.
voice-channel-closed = The voice channel was closed.
closed-signed-out = This session was ended elsewhere; sign in again.
closed-session-expired = This session expired; sign in again.
closed-kicked = An admin signed you out everywhere. Sign in again to continue.
closed-unreachable = The server stopped hearing from this client.
closed-client-shutdown = This session is over, as asked.
closed-server-stopping = The server is restarting; connect again in a moment.
closed-moved = This account joined voice from another device.
closed-not-allowed = Your roles on this server changed, so you left voice.
closed-channel-deleted = The voice channel was deleted, so you left it.
closed-taken-out = An admin took you out of the voice channel.
closed-over-rate = This device sent more than the server allows, so it left the voice channel.
closed-password-changed = Your password was changed on another device. Sign in with the new one.
closed-password-reset = An admin reset your password. Sign in with the temporary password they gave you.
closed-account-deleted = This account was deleted from this server.
closed-banned = An admin banned this account from this server.
picture-unreadable = That picture can’t be read. Choose another one.
file-signed-out = This session is not signed in; sign in again.
cant-change-picture = You can’t change this server’s picture.
picture-gone = That picture is no longer on the server.
not-a-picture = That isn’t a PNG, JPEG or WebP picture.
server-full = The server has no room for more files. Ask whoever runs it.
too-many-requests = Too many requests; try again in a moment.
# The server holds as many connections as it may.
server-busy = The server is busy; try again in a moment.
server-couldnt-do-it = The server couldn’t do that. Try again later.
file-not-taken = The server didn’t take that file.
# The composer's strip: more files picked than a message carries, or one whose name a server cannot take.
too-many-files = A message carries at most 10 files.
file-name-refused = That file’s name can’t be sent. Rename it and try again.
no-file-picker = This system has no file picker.
no-save-dialog = This system has no way to ask where to save a file.
link-not-opened = That link couldn’t be opened.
file-unreadable = That file can’t be read.
# A file sent from this device, or saved to it, that could not be read or written there.
file-on-device = That file couldn’t be read or written on this device.
cant-attach-files = You can’t attach files in this channel.
cant-add-emoji = You can’t add emoji to this server.
file-gone = That file is no longer on the server.
emoji-too-large = That picture is over { $size }. Choose a smaller one.
# An attachment over the most the server takes for one file, as its host set it: "50 MB".
file-too-large = This file is over { $size }.
file-too-large-unsaid = This file is larger than this server takes.
# The account's files at the most this server keeps for one account, as its host set it: "1 GB".
files-used = You’ve used your { $size } for files on this server.
files-used-unsaid = You’ve used all the room this server keeps for your files.
screen-unreachable = The screen could not be reached.
screen-needs-copy = This screen cannot be shared without copying it, which is not done.
screen-size-unshareable = This screen has a size that cannot be shared.
capture-stopped = Sharing the screen stopped working.
capture-needs-pipe-wire = Sharing the screen needs PipeWire, which this device does not have.
share-ended = The screen is no longer being shared.
screen-silent = The screen sent no picture in that time.
encoder-refused-size = This device’s video encoder would not take the screen at this size.
video-hardware-closed = This device’s video hardware did not open.
screen-resized = The screen changed size while it was being shared.
# Text: a share that could not begin.
video-hardware-needs-copy = This device’s video hardware cannot take the screen’s pictures without copying them, which is not done.
encode-stopped = Encoding the screen stopped working.
share-sound-needs-pipe-wire = The share’s sound needs PipeWire, which this device does not have.
share-sound-needs-newer-windows = This version of Windows cannot share sound.
share-sound-stream = The share’s sound could not start.
share-sound-stopped = The share’s sound stopped; Chatkord is trying to bring it back.
camera-denied = Your system’s privacy settings don’t let Chatkord use the camera.
camera-busy = Couldn’t turn on the camera. Another app may be using it.
camera-gone = The camera was disconnected.
camera-format = This camera offers no picture Chatkord can send.
camera-unavailable = Cameras aren’t available on this system yet.
voice-offer = The server’s voice details could not be read.
voice-engine = Your voice could not be set up on this device.
call-faulted = This call stopped after a fault in Chatkord; reconnecting to the server.
engine-stalled = Your device did not answer in time.
sound-stopped = Sound stopped working. Restart Chatkord.
already-sending = That is already being sent.
sound-unavailable = Sound could not be started on this device.
device-missing = That sound device is not here.
device-refused = That sound device could not be used.
picker-no-portal = Screen sharing is not available: no screen-share portal is installed.
picker-needs-newer-windows = Sharing the screen needs a newer version of Windows.
picker-unavailable = Screen sharing is not available on this system yet.
nothing-chosen = Nothing was chosen to share.
picker-failed = The desktop could not start the share.
cannot-decode = This device cannot decode the stream.
screen-not-carried = The server can’t send you this stream now.
share-sound-not-carried = The server can’t send you this stream’s sound now.
# Over a watched stream's card, above its name, while this computer cannot decode it as fast as it comes.
cant-keep-up = This computer can’t keep up with this stream.
# In a watched stream's popover, a warning, while the same holds.
cant-keep-up-note = This computer can’t keep up with this stream

## Sentences that name something: a server, a person, a number.

# Under the server's picture row: what it takes, from the bounds the server keeps.
not-found = No server answers at “{ $host }”. Check the spelling.
not-chatkord = “{ $host }” isn’t a Chatkord server.
# A call whose media never connected, or whose voice address named nothing this device may reach.
voice-unreached = Voice couldn’t reach { $host } on port { $port }; a firewall may block it.
# What this call has sent, and has received, as a size.
sent-size = ↑ { $size }
received-size = ↓ { $size }
# A monitor in our picker, by its place in the list (from 1).
screen-numbered = Screen { $number }
# The heading of the connection popover's graph: how far back it reaches.
round-trip-over = Round trip in ms, last { $minutes } minutes
# A mark under the connection popover's graph: how many minutes back.
minutes-ago = { $minutes } min
# The connection popover's key frames of this account's share: made by its encoder, asked for by its viewers.
key-frames-count = { $made } made · { $asked } asked
# A watched screen's popover: how often its picture froze, and the longest, in seconds with one decimal, the catalog's own separator.
froze = { $times ->
    [one] Froze once
   *[other] Froze { $times } times
}, the longest { $seconds } s
# A watched screen's popover: frames its decoder refused.
not-decoded = { $frames ->
    [one] 1 frame not decoded
   *[other] { $frames } frames not decoded
}
# A day in UTC, as an invite stops working: the day of the month, and the month from 1.
day-month = { $day } { $month ->
    [1] January
    [2] February
    [3] March
    [4] April
    [5] May
    [6] June
    [7] July
    [8] August
    [9] September
    [10] October
    [11] November
   *[12] December
}
# When an invite stops working.
until = · until { day-month }
youll-own = You’ll own { $server }.
create-title = Create your account on { $server }
sign-in-title = Sign in to { $server }
# Spaces in a username, and the name with dots instead.
spaces = No spaces. Try { $name }
# A username, an invite's note or a channel's name longer than any may be.
no-more-than = No more than { $most } characters.
taken = The name “{ $name }” is taken on { $server }. Is it yours?
# A new password's placeholder: the fewest characters it may have.
at-least = At least { $fewest } characters
to-go = { $left } more to go
no-answer = { $server } didn’t answer. Check your connection and try again.
already-have = Already have an account on { $server }?
wrong = That username or password doesn’t match on { $server }.
forgot = Only the owner or an admin of { $server } can reset it. Ask one of them for a temporary password, then sign in with it.
creating = Creating your account on { $server }…
signing-in = Signing in to { $server }…
connecting = Connecting to { $server }…
new-password-body = You signed in with a temporary password for { $server }. Choose your own to continue.
welcome = Welcome to { $server }.
seen-as = Others see you as “{ $name }”. You can change your name in Profile.
you-own = You own { $server }
# The title of the set-up dialog a server's owner sees first.
set-up = Set up { $server }
# Why a remembered account must sign in again: its session ran out.
sign-in-expired = Your sign-in has expired. Sign in again to continue.
# The same: it signed out on another device.
signed-out-elsewhere = You were signed out of { $server } on another device. Sign in again to continue.
# The same: its account was deleted.
account-deleted-on = Your account on { $server } was deleted. To come back, you’ll need a new invite.
# A server that does not answer, as its panel's title.
not-answering = { $server } isn’t answering
# When the next try is, in whole seconds.
trying-in = You’re still signed in. { $seconds ->
    [0] Trying again now.
    [one] Trying again in 1 second.
   *[other] Trying again in { $seconds } seconds.
} Your other servers keep working.
lost = Connection to { $server } lost. Reconnecting…
# Back after a drop that left this call.
back-after-drop = You’re back. You left { $channel } when the connection dropped.
rejoin = Rejoin { $channel }
# Where the account is: in this voice channel.
in-channel = In { $channel }
# A voice channel's row: a click joins it, or moves the call to it.
join-channel = Join { $channel }
# The row of the channel this account's call is in: a click opens the call.
open-channel = Open { $channel }
# A microphone or speaker that is not connected, and the device used instead.
sound-gone = “{ $was }” isn’t connected, so Chatkord uses “{ $now }”.
# The same, and the system's default is used instead.
sound-gone-to-default = “{ $was }” isn’t connected, so Chatkord uses the default.
# A microphone or speaker that was not connected and is again.
sound-back = “{ $name }” is connected again.
# The title of a call whose channel was deleted.
channel-was-deleted = { $channel } was deleted
# The title of a call that could not be made.
could-not-join = Couldn’t join { $channel }
raised-on = You have a higher role on { $server } now
no-longer-on = You’re no longer on { $server }
# The heading of the panel of a server that presented another key than the one this device trusts.
identity-changed = The identity of { $server } has changed
# Why a server's panel asks for the password: this device signed out of it.
signed-out-here = You signed out of { $server } on this device.
sign-out-of = Sign out of { $server }
sign-out-title = Sign out of { $server }?
remove-title = Remove { $server } from this device?
remove-action = Remove { $server }
# The deleted account's panel's one way on.
remove-from-here = Remove { $server } from this device
leave-server = Leave { $server }
leave-title = Leave { $server }?
leave-body = This deletes your account on { $server }. To come back, you’ll need a new invite.
# A confirmation's note when the server holds this window's call: the channel.
also-leaves-call = You’ll also leave the call in { $channel }.
# Closing the window during a call asks first: the call's channel.
quit-leaves-call = Closing Chatkord leaves the call in { $channel }.
copied = Address of { $server } copied
you-left = You left { $channel }
# An administrator took the account out of a channel.
out-of = You’re out of { $channel }
# What an administrator did.
taken-out-of = An admin took you out of { $channel }.
# The same, said while another server is open: the channel and the server it is on.
taken-out-on = An admin took you out of { $channel } on { $server }.
# Text: an administrator muted this account for everyone: what its own microphone is once they undo it.
held-own-muted = An admin muted you for everyone. When they undo it, your microphone is muted, as you set it.
# Text: the same, its microphone on.
held-own-on = An admin muted you for everyone. When they undo it, your microphone is on.
# The microphone button's tip while an administrator holds this account muted.
held-tip = An admin muted you for everyone
already-listed = { $server } is already in your list.
open-server = Open { $server }
# The invite dialog's title: the server.
invite-to = Invite people to { $server }
# How many an invite admits.
people = { $count ->
    [one] 1 person
   *[other] { $count } people
}
# A watched screen's heading: whose it is.
screen-of = { $name }’s stream
# The card of a watched screen playing in a window of its own.
screen-out = { $name }’s stream is in its own window
# A watched screen over the whole display, as a screen reader names it: whose it is.
full-screen-of = { $name }’s stream, full screen
# The popover of how loud a shared screen's sound plays here, and its row in Audio controls: whose screen it is.
sound-from-screen = Sound from { $name }’s stream
# A watched screen's picture while its connection comes back.
reconnecting-to = Reconnecting to { $name }’s stream…
# One's own share's sound, from a window's app.
sound-of = Sound: { $name }
# One's own screen sent smaller than it is: the screen's size.
scaled-from = Scaled from { $size }
# One's own screen sent below what was chosen because the rate chosen is too low for it: what it was, and the limit.
reduced-ceiling = Reduced from { $from }: { $rate } limit
# The same, because the screen's content needs more than the rate: what it was, and the rate.
reduced-busy = Reduced from { $from }: content exceeds { $rate }
# The same, under an automatic rate: what it was.
reduced-busy-connection = Reduced from { $from }: content exceeds the connection
# The same, because the encoder could not keep up: what it was.
reduced-encoder = Reduced from { $from }: encoder overloaded
# The reason the system is given for keeping the device awake during a call, where it lists what does.
keeping-awake = In a call
# How many days an invite works.
for-days = { $count ->
    [one] 1 day
   *[other] { $count } days
}
# What a link just made admits: how many, and until when.
invite-terms = { $people } · { $role } · until { day-month }
# The same, and its note.
invite-terms-noted = { $people } · { $role } · until { day-month } · { $note }
# How many times an invite was used, of how many it admits.
used = { $used } of { $of }
# The server keeps as many working invites as it may.
too-many-invites = There are already { $most } working invites. Delete one first.
# The custom number of people is not one an invite admits: the most it may.
uses-out-of-range = Pick a number from 1 to { $most }.
# The server holds as many channels as it may.
too-many-channels = There are already { $most } channels. Delete one first.
rename-title = Rename { $name }
delete-channel-title = Delete { $name }?
delete-channel-action = Delete { $name }
# How many are in a voice channel's call.
in-the-call = { $count } in the call
# A member's row under their name: their username and highest role, on this account's own row too, and banned.
placed = @{ $handle } · { $place }
placed-you = @{ $handle } · { $place } · you
handle-only = @{ $handle }
handle-you = @{ $handle } · you
handle-banned = @{ $handle } · Banned
# The members page's two lists, by how many each holds.
members-count = Members ({ $count })
banned-count = Banned ({ $count })
# A search of the members that finds no one: what is typed.
no-one-matches = No one matches “{ $query }”.
# The menus on a member's row: their display name.
roles-for = Roles for { $name }
more-for = More for { $name }
kick-member = Kick { $name }
kick-title = Kick { $name }?
kick-body = { $name } is signed out on every device. Their account stays, so they can sign in again; to keep them out, ban them.
kicked = { $name } was signed out everywhere.
cant-kick = You can’t kick { $name }.
ban-member = Ban { $name }
ban-title = Ban { $name }?
ban-body = { $name } is signed out everywhere and can’t sign in to { $server } until they’re unbanned. Their messages stay.
unban-member = Unban { $name }
unban-title = Unban { $name }?
unban-body = { $name } can sign in to { $server } again with their own password.
cant-ban = You can’t ban { $name }.
cant-unban = You can’t unban { $name }.
# A change to a role the server refused as not allowed: the role's name.
cant-change-role = You can’t change { $role }.
# A role at or above this account's place: its name.
role-locked = { $role } is at or above your own place, so only someone higher can change it.
# Where a new role starts: the default role's name.
create-role-body = It starts just above { $role } and adds nothing until you give it words.
delete-role-title = Delete { $role }?
delete-role-action = Delete { $role }
# The server holds as many roles as it may.
too-many-roles = A server holds at most { $most } roles.
cant-change-roles = You can’t change { $name }’s roles.
cant-reset = You can’t reset { $name }’s password.
reset-title = Reset { $name }’s password?
reset-body = { $name } will be signed out everywhere. You’ll get a temporary password to give them.
temporary-for = Temporary password for { $name }
hand-over = Share it with { $name } privately, in person or in a message. They choose their own password when they sign in.
transfer-title = Make { $name } the owner of { $server }?
transfer-body = You’ll keep the role just below theirs, if there is one. Only { $name } can give ownership back.
transfer-action = Make { $name } the owner
# A share that ended without being stopped, or could not begin, and why.
share-stopped = Your share stopped. { $why }
# A picture over the most a server takes, in megabytes.
picture-too-large = That picture is over { $size }. Choose a smaller one.
# A picture over the most pixels on a side a server takes.
picture-too-wide = That picture is over { $pixels } pixels on a side. Choose a smaller one.

# A text channel: the side pane's heading over the text channels.
text-section = Text
# The composer's name, and what it shows while empty.
message-channel = Message #{ $channel }
send = Send
# Send's tooltip while the composer holds more than a message may.
too-long-to-send = Too long to send
# In place of the composer where this account's roles do not let it send.
cannot-send = You can’t send messages in #{ $channel }.
# A deleted message's place in its run.
message-deleted = Message deleted
# After an edited message's words.
edited = (edited)
# The author of a message whose account is gone.
former-member = Former member
# The day lines between messages.
today = Today
yesterday = Yesterday
day-line = { $weekday ->
    [1] Monday
    [2] Tuesday
    [3] Wednesday
    [4] Thursday
    [5] Friday
    [6] Saturday
   *[7] Sunday
} { day-month }
# At the top of the list while older messages are read.
loading-older = Loading older messages…
# Over the composer while the newest messages are not shown.
viewing-older = You’re viewing older messages
jump-to-present = Jump to present
# A channel's beginning, at the top of its list: its welcome, and under it what this is. A voice channel's chat
# says the same of itself, by its name alone.
channel-welcome = Welcome to #{ $channel }!
channel-start = This is the start of the #{ $channel } channel.
voice-chat-welcome = Welcome to { $channel }!
voice-chat-start = This is the start of the { $channel } channel.
# A message's menu.
copy-text = Copy text
edit = Edit
# The field a message is edited in, and the line under it: the two words are links.
edit-message = Edit message
edit-escape = Esc to
edit-enter = · Enter to
edit-cancel = cancel
edit-save = save
# A hidden spoiler in a message, for a screen reader, and one shown with its words.
spoiler-hidden = Spoiler, hidden. Press to show it.
spoiler-shown = Spoiler: { $text }
# The tools over a hovered message, and its menu's pin.
react = React
reply = Reply
more-actions = More
pin-message = Pin message
unpin-message = Unpin message
# A message's reactions: the way to add one, and a chip as a screen reader says it, one's own marked.
add-reaction = Add a reaction
reaction-chip = { $emoji }: { $count }
reaction-chip-yours = { $emoji }: { $count }, yours among them
# The bar over the composer while replying: the words before the name, whether the author is mentioned, and its way
# out.
replying-to = Replying to
mention-author = Mention the author
mention-on = On
mention-off = Off
cancel-reply = Cancel reply
# The quoted line over a reply whose original is gone.
original-deleted = Original message was deleted
# The channel's pinned messages, from its header.
pinned-messages = Pinned messages
jump = Jump
unpin = Unpin
no-pins = Nothing is pinned here yet.
# The emoji picker: its search, the skin tone of its hands, its groups and the tones.
find-emoji = Find an emoji
skin-tone = Skin tone
emoji-groups = Emoji groups
emoji-recent = Recent
emoji-group-smileys = Smileys
emoji-group-people = People
emoji-group-nature = Nature
emoji-group-food = Food and drink
emoji-group-travel = Travel and places
emoji-group-activities = Activities
emoji-group-objects = Objects
emoji-group-symbols = Symbols
emoji-group-flags = Flags
tone-default = Default
tone-light = Light
tone-medium-light = Medium light
tone-medium = Medium
tone-medium-dark = Medium dark
tone-dark = Dark
no-emoji-match = No emoji match that.
# The composer's chooser after @, # and :, its groups and the notes beside @everyone and @here.
mentions = Mentions
chooser-members = Members
chooser-roles = Roles
chooser-everyone = Everyone
chooser-channels = Channels
everyone-note = Everyone who can see #{ $channel }
here-note = Everyone online who can see it
emoji-matching = Emoji matching :{ $query }
# Files: the composer's way to attach, its strip of files to send, a file's ways out and the drop over the channel.
attach-file = Attach a file
files-to-send = Files to send
cancel-upload = Cancel upload
remove-file = Remove { $name }
save-file = Save { $name }
drop-to-attach = Drop to attach
# In place of the words of a message that is files alone, where it is quoted on one line.
files-alone = Attachment
drop-note = Files go to #{ $channel }, up to { $most } each.
# Server settings › Emoji.
emoji = Emoji
upload-emoji = Upload emoji
emoji-note = Everyone on { $server } can use these in messages and reactions. PNG, JPEG or WebP, up to 256 KB.
added-by = Added by { $name }
rename-emoji = Rename :{ $name }:
delete-emoji = Delete :{ $name }:
emoji-name-rule = Use 2 to 32 letters, digits or underscores.
emoji-full = { $server } has { $most } emoji, the most a server holds. Delete one to upload another.
# A new emoji's name, typed once its picture is up.
name-emoji = Name the new emoji
# The confirmation before an emoji is deleted.
delete-emoji-title = Delete :{ $name }:?
delete-emoji-body = Messages and reactions that used it show :{ $name }: as text instead. This can’t be undone.
delete-emoji-action = Delete emoji
# The confirmation before a message is deleted: its heading, a line of what it says, and a note.
delete-message-title = Delete { $author }’s message?
delete-message-line = “{ $line }”
delete-message-note = This can’t be undone.

# A list of names said together: a name after the ones before it.
list-comma = { $first }, { $rest }
# A person who is not signed in on any device, in their popover.
presence-offline = Offline
# The members column's two lists, by how many each holds.
members-online = Online — { $count }
members-offline = Offline — { $count }
# The channel header's toggle of the members column.
show-members = Show members
hide-members = Hide members
# The grip on the edge of the members column that resizes it.
members-width = Members width
# The call header's button that shows the call's channel's messages beside it, or hides them.
show-call-chat = Show chat
hide-call-chat = Hide chat
# The grip on the edge of a call's chat column that resizes it.
call-chat-width = Chat width
# A moderation the server refused because the person was no longer in a call: their name.
not-muted-left = { $name } couldn’t be muted for everyone: they left the call.
not-taken-out-left = { $name } couldn’t be taken out of voice: they left the call.
# The side pane's list of channels, for a screen reader.
channels-in-order = Channels, in the server’s order
# A category's "+" in the side pane: its name.
create-channel-in = Create a channel in { $category }
# The items of a channel's and a category's menus.
edit-channel = Edit channel
rename-channel = Rename channel
delete-channel = Delete channel
create-category = Create category
rename-category = Rename category
delete-category = Delete category
# A text channel's name as a title says it.
rename-text-title = Rename #{ $name }
delete-text-title = Delete #{ $name }?
delete-text-action = Delete #{ $name }
# What deleting a text channel takes with it.
delete-text-body = Its messages go with it. This can’t be undone.
create-category-title = Create a category
category-example = New category
# Deleting a category deletes its channels: how many.
delete-category-body = { $count ->
    [0] It holds no channels, so only its name goes.
    [one] Its channel and its messages are deleted too.
   *[other] Its { $count } channels and their messages are deleted too.
}
# A move of a channel or a category refused twice because the list changed meanwhile.
move-refused = The channel list changed. Try the move again.
# Making a channel: its kind, its category and who sees it or writes in it.
channel-kind = Kind
kind-of-channel = Kind of channel
kind-text = Text
kind-voice = Voice
category-label = Category
text-channel-example = new-channel
private-channel = Private channel
private-channel-line = Only the roles and members below, and the owner, see it.
who-can-see = Who can see it
read-only = Read-only
read-only-line = Everyone reads it; only the roles and members below, and the owner, write in it.
who-can-write = Who can write
add-role-or-member = Add a role or member
# This account, first in a list of who sees or writes in a channel.
you-listed = You
remove-target = Remove { $name }
find-a-member = Find a member
every-member-listed = Every member you can choose is listed.
# Server settings › Channels.
channels-drag-line = Drag a channel or a category, or press Alt and an arrow key, to change the order.
channel-name = Channel name
category-name = Category name
access = Access
you-stay = You stay in it as long as you’re listed.
would-not-see = You would no longer see this channel.
advanced-permissions = Advanced permissions
# Text: the note above the advanced permissions.
advanced-line = Allow or deny a word in this channel for a role or a member. Inherit keeps what their roles give.
whose-words = Whose words
add-to-channel = Add to this channel
add = Add
remove = Remove
remove-from-channel = Remove { $name } from this channel
deny = Deny
inherit = Inherit
allow = Allow
only-words-you-hold = You can change only the words you hold.
overwrite-refused = That change wasn’t saved: you no longer hold Manage roles.
colour-refused = That colour wasn’t saved, so the old one is back.
# The lock on a private channel's icon.
private-mark = Private
# A private channel's start, under its welcome.
private-channel-start = This is the start of the private #{ $channel } channel.
# The server holds as many categories as it may.
too-many-categories = There are already { $most } categories. Delete one first.
# A mute's duration, in the server's Notification settings and a channel's Mute menu.
mute-for15-minutes = For 15 minutes
mute-for1-hour = For 1 hour
mute-for8-hours = For 8 hours
mute-for24-hours = For 24 hours
mute-until-turned-on = Until I turn it back on
# Who is writing in a channel, under its composer.
one-typing = { $name } is typing…
two-typing = { $first } and { $second } are typing…
several-typing = Several people are typing…
# A server's Notification settings dialog and a channel's menus.
notification-settings = Notification settings
notifications-from = Notifications from { $server }
mute-server = Mute { $server }
mute-server-note = No unread marks or notifications unless you’re mentioned.
unmute-server = Unmute { $server }
# The dialog's Mute row's button, which opens its durations, and the same once muted.
mute-action = Mute
unmute-action = Unmute
notify-me-about = Notify me about
notify-all = All messages
notify-mentions = Only mentions
notify-mentions-note = @everyone, @here, your roles and you
notify-nothing = Nothing
suppress-everyone = Suppress @everyone and @here
suppress-roles = Suppress role mentions
mute-channel = Mute channel
unmute-channel = Unmute channel
use-server-default = Use server default
# A channel's row in the list, for a reader of the screen.
channel-unread = { $channel }, unread
channel-mentions = { $channel }, { $count ->
    [one] 1 mention
   *[other] { $count } mentions
}
channel-muted-mentions = { $channel }, muted, { $count ->
    [one] 1 mention
   *[other] { $count } mentions
}
# A call's header: its channel's messages beside it.
show-chat = Show chat
show-chat-unread = Show chat, unread messages
hide-chat = Hide chat
close-chat = Close chat
# An upload under way in the composer's strip: "4.1 of 12 MB".
upload-progress = { $sent } of { $size }
# A file card's note: its size, then its kind from its name's extension: "1.2 MB · PDF".
file-note = { $size } · { $kind }
# A pin's time in the pins panel: its day as the list's day lines say it, then the time: "Today · 21:06".
pinned-at = { $day } · { $time }
# A voice channel's composer, beside its call: its name, and what it shows while empty.
message-voice-channel = Message { $channel }
# The picture viewer's note: its size in pixels, then in bytes: "1600 × 1200 · 2.4 MB".
picture-note = { $dimensions } · { $size }
# The line in a channel's list over the first message not read when it was opened.
new-messages = New messages
# The mark on that line.
new-mark = New
# The picture viewer while the picture could not be had.
picture-not-fetched = This picture couldn’t be loaded.
# The side pane's two rows above the channels, and the lists they open.
threads = Threads
direct-messages = Direct messages
no-threads-yet = No threads yet
no-threads-body = Threads you start, reply in or are mentioned in show here.
# The empty state of a server with no category, in the pane and on the Channels page.
no-categories-yet = No categories yet
no-categories-body = Channels live in categories.
no-direct-messages-yet = No direct messages yet
no-direct-messages-body = Start one from a member’s profile: click their name, then Message.
# The button on a member's popover that opens a conversation with them.
message = Message
# The composer's placeholder in a conversation: the other person's name.
message-person = Message { $name }
# An entry of a message's menu, and the header of the pane a thread opens in.
reply-in-thread = Reply in thread
thread = Thread
follow-thread = Follow thread
unfollow-thread = Unfollow thread
# A message's thread chip and a thread's row.
reply-count = { $count ->
    [one] 1 reply
   *[other] { $count } replies
}
# A time, said by the clock on the day it is, else by the day.
last-reply = last { $when }
# A preview that is this account's own.
you-colon = You:
# In place of the composer of a conversation whose other member is gone.
no-longer-here = { $name } is no longer on this server.
# Over the first message of a conversation.
conversation-start = This is the start of your conversation with { $name }.
# Settings › Notifications: popups and do not disturb.
show-popups = Show desktop notifications
show-popups-note = When a message would alert you and Chatkord isn’t in front.
do-not-disturb = Do not disturb
# Settings › Notifications: do not disturb is the account's status; its note says whether it is on, and its button opens
# the status.
do-not-disturb-on = On: no popups or sounds on any of your devices.
do-not-disturb-on-until = On until { $when }: no popups or sounds on any of your devices.
do-not-disturb-on-until-tomorrow = On until tomorrow { $when }: no popups or sounds on any of your devices.
do-not-disturb-off-note = Your status: choose Do not disturb in it, and none of your devices alerts.
choose-status = Choose a status
# The account's status, as its popover offers it and the account bar says it, and how long a status lasts.
status = Status
status-away = Away
status-invisible = Invisible
status-for-hour = For 1 hour
status-for-eight-hours = For 8 hours
status-until-tomorrow = Until tomorrow
status-until-off = Until I turn it off
# Under the status chosen while it has an end: a time today, else tomorrow's.
status-until = Until { $when }
status-until-tomorrow-at = Until tomorrow { $when }
# A desktop notification: its title names the writer and the server, or the writer alone in a conversation.
popup-title = { $author } in { $server }
popup-body-channel = #{ $channel } · { $text }
popup-body-direct = { $server } · direct message: { $text }
popup-body-thread = Thread in #{ $channel } · { $text }
# Message search: the header's field, named for the server, and the panel its results hang in.
search-messages = Search { $server }
search-results = Search results
search-nothing = Nothing found for “{ $query }”.
# Text: what a search reaches, under its field.
search-finds = Search finds whole words and their starts in every channel and conversation you can read, and files by name.
search-failed = Search didn’t answer.
search-waiting = Searching…
show-more = Show more
# Where a found message was written, when it is a reply in a thread.
search-in-thread = #{ $channel } › thread
# Link previews: the card above the composer while a draft's link is fetched, the card under a message, and the
# setting that turns them off.
remove-preview = Remove preview
preview-waiting = Getting a preview of { $host }…
open-preview = Open { $title }, { $host }
show-previews = Show a preview of links I send
# Text: the switch's hint.
show-previews-note = Chatkord opens a link from this device to read its title and picture when you type it. Everyone else sees what you sent.
# Server settings › Overview: the features an administrator may switch off, and what the server does about each.
features = Features
switch-conversations-line = Members write to each other one to one.
switch-threads-line = Replies to a message gather in a thread of its own.
switch-link-previews = Link previews
switch-link-previews-line = A link sent shows its page’s title and picture.
switch-search = Search
switch-search-line = Messages and files, found from every page with Ctrl+K.
switch-welcome = Welcome note
switch-welcome-line = A new member is greeted, and shown how to change their name.
switches-note = A feature turned off is hidden for everyone, and the server refuses it.
# Server settings › Overview › Hosting: where people reach the server, how much it keeps and how much it lets in.
hosting = Hosting
public-address = Public address
# Text: the field's hint.
public-address-line = Where people reach this server, with its port if it isn’t 443. Invite links and calls use it.
public-address-empty = Calls wait for it: until it is set, no one can join a call.
public-address-refused = Write a host name or an address, then a port if it isn’t 443.
hosting-files-line = An upload past either is refused.
hosting-attachment-max = Largest file
hosting-account-files-max = Per-person storage
no-limit = No limit
unit-mb = MB
whole-number = A whole number.
hosting-most = No more than { $most } MB.
hosting-least = At least { $least } MB.
hosting-within = No more than the per-person storage, { $most } MB.
# Server settings › Webhooks: an app that posts in one channel under its own name.
webhooks = Webhooks
new-webhook = New webhook
# Text: the note at the top of the page.
webhooks-line = A webhook lets an app such as Grafana or Home Assistant post in one channel. Anyone who has its URL can post with it.
webhook-name-head = Name
webhook-channel-head = Channel
webhook-made-by-head = Made by
webhook-made-head = Made
no-webhooks = No webhooks yet. Make one to let an app post in a channel.
regenerate-url = Regenerate URL
delete-webhook = Delete webhook
new-webhook-line = It posts in one channel under its own name, marked as an app.
webhook-name-label = Name
webhook-channel-label = Channel
may-mention-everyone = May mention everyone
may-mention-everyone-line = Its posts may use @everyone and @here.
create-webhook = Create webhook
webhook-url = Webhook URL
webhook-url-once = Copy it now: it won’t be shown again.
webhook-example = Server alerts
regenerate-webhook-body = The URL it has now stops working at once. You get a new one to paste into the app.
delete-webhook-body = The app can’t post with it any more. What it posted stays.
# The badge beside the name of an app's post.
app-badge = APP
# The dialogs about one webhook, by the name it posts under.
webhook-url-for = Webhook URL for { $name }
webhook-posts-in = Paste it into the app that posts. It posts in #{ $channel }.
regenerate-webhook-title = Regenerate the URL of { $name }?
delete-webhook-title = Delete { $name }?
# A webhook's post: the name its app gave.
webhook-posted-by = Posted by the webhook “{ $name }”
# An app's About, when the channel of the post is known.
webhook-posted-in = Posted by the webhook “{ $name }” in #{ $channel }.
# Search from every page of a server: the bar's hint, the layer and what it finds.
search-shortcut = Ctrl+K
search-start = Search messages and files
search-placeholder = Messages, and files by their names
no-messages-found = No messages found.
no-files-found = No files found.
files-heading = Files
jump-to = Jump to { $name }
found-file = { $size } · { $place } · { $day }
# A camera in a window of its own, and on its card while it is there.
camera-of = { $name }’s camera
camera-out = { $name }’s camera is in its own window
webhook-name-needed = Give it a name.
webhook-channel-needed = Choose a channel.

### How a measured number reads. A language sets its own separators, units and order; `$n` arrives already written
### with the group separator, so no message formats a number itself.

# Text: the mark between a whole part and its tenths.
figure-decimal = {"."}
# Text: the mark between every three digits of a whole number.
figure-group = {","}
figure-bits = { $n } bit/s
figure-kbits = { $n } kbit/s
figure-mbits = { $n } Mbit/s
figure-b = { $n } B
figure-kb = { $n } kB
figure-mb = { $n } MB
figure-gb = { $n } GB
figure-ms = { $n } ms
# A round trip too short for the engine to resolve.
figure-under-ms = under 1 ms
# Where nothing was measured.
figure-none = —
figure-percent = { $n } %
# A picture's size: its width by its height.
figure-dimensions = { $w } × { $h }
figure-fps = { $n } fps
# A picture's short side in pixels: 1080p.
figure-p = { $n }p
# A message's time on the clock, hour and minute each two digits.
figure-clock = { $hour }:{ $minute }
# How much a draft holds of the most a message may.
figure-of-most = { $count } / { $most }
