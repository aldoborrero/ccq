Output assets/cli-demo.gif

Set Shell bash
Set FontSize 14
Set TypingSpeed 50ms
Set Width 1000
Set Height 600
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

# Index
Type "ccq index"
Enter
Sleep 3s

# Search
Type "ccq search 'nix flake'"
Enter
Sleep 3s

# Verbose search with context
Type "ccq search 'nix flake' -v --context 1 --limit 5"
Enter
Sleep 4s

# List sessions
Type "ccq sessions"
Enter
Sleep 3s

# Stats
Type "ccq stats"
Enter
Sleep 3s
