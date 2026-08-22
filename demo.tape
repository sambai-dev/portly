# VHS demo tape — generates docs/demo.gif
#
#   vhs demo.tape
#
# The recording runs the real binary against the real machine, so the script
# deliberately never confirms a kill: it arms the action (x) to show the
# confirm banner, then cancels (esc).

Output docs/demo.gif
Set FontSize 20
Set Width 1100
Set Height 640
Set Theme "Dracula"
Set Padding 24

Type "portly"
Enter
Sleep 2500ms

# filter for node dev servers
Type "/node"
Sleep 1200ms
Esc

# cycle sort modes
Type "s"
Sleep 400ms
Type "s"
Sleep 800ms

# show the two-step kill flow without executing it
Down
Sleep 300ms
Type "x"
Sleep 1500ms
Esc

# logs pane (docker container if present)
Down
Sleep 200ms
Type "l"
Sleep 2500ms
Esc

# help overlay
Type "?"
Sleep 1800ms
Esc
Sleep 600ms
