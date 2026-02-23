Output assets/tui-demo.gif

Set Shell bash
Set FontSize 14
Set TypingSpeed 50ms
Set Width 1200
Set Height 700
Set Theme "Catppuccin Mocha"
Set Padding 15
Set WindowBar Colorful
Set WindowBarSize 40

Hide
Type `export PS1='$ '`
Enter
Type "clear"
Enter
Sleep 500ms
Show

# Launch TUI
Type "ccq tui"
Enter
Sleep 2s

# Navigate sessions
Type "j"
Sleep 500ms
Type "j"
Sleep 500ms
Type "j"
Sleep 500ms

# Select a session
Enter
Sleep 1s

# Navigate messages
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 1s

# Toggle maximize
Type "m"
Sleep 2s

# Navigate in maximized view
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 1s

# Toggle maximize off
Type "m"
Sleep 1s

# Go back to sessions
Escape
Sleep 500ms

# Open search
Type "/"
Sleep 500ms

# Type a search query
Type "rho"
Sleep 2s

# Confirm search
Enter
Sleep 1s

# Navigate search results
Type "j"
Sleep 500ms
Type "j"
Sleep 500ms
Enter
Sleep 1s

# Jump to next hit
Type "n"
Sleep 800ms
Type "n"
Sleep 800ms

# Filter messages within conversation
Type "/"
Sleep 500ms
Type "build"
Sleep 2s
Escape
Sleep 1s

# Show help
Type "?"
Sleep 3s

# Dismiss help
Type "q"
Sleep 500ms

# Quit
Type "q"
Sleep 1s
