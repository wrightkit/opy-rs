settings {
    "main": {
        "zzUnknownKey": 42
    },
    "gamemodes": {
        "ffa": {
            "enabled": true
        }
    },
    "heroes": {
        "allTeams": {
            "ana": {
                "ability3Cooldown%": 0
            }
        }
    }
}

rule "settings passthrough":
    @Event global
    pass
