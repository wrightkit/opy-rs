globalvar g

enum E:
    A
    settings {
        "gamemodes": {"ffa": {"scoreToWin": 7}}
    }

settings {
    "gamemodes": {"ffa": {"scoreToWin": 9}}
}

rule "probe":
    g = E.settings
