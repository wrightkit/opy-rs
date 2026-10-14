settings {
    "main": {
        "description": "opy-rs#545 general keys inside general and a hero"
    },
    "gamemodes": {
        "skirmish": {}
    },
    "heroes": {
        "allTeams": {
            "general": {
                "health%": 100,
                "passiveHealthRegen": false
            },
            "widowmaker": {
                "passiveHealthRegen": false
            }
        },
        "team1": {
            "general": {
                "damageReceived%": 50
            }
        },
        "team2": {
            "general": {
                "enableHeadshotsOnly": true
            }
        }
    }
}

rule "setup":
    @Event global
    pass
