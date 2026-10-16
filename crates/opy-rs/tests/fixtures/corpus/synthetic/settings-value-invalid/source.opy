settings {
    "main": {
        "description": "wrong-typed settings value"
    },
    "gamemodes": {
        "ffa": {
            "enabled": true,
            "scoreToWin": "abc"
        }
    }
}

rule "settings value":
    @Event global
    pass
