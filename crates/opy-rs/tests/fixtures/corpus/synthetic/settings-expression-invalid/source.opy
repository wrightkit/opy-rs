globalvar g

settings {
    "main": {
        "modeName": g
    },
    "gamemodes": {
        "ffa": {
            "enabled": true
        }
    }
}

rule "settings expression":
    @Event global
    pass
