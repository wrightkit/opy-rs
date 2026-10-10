rule "body":
    @Event global
    if 1 > 0:
        settings {
            "gamemodes": {"ffa": {"scoreToWin": 7}}
        }
