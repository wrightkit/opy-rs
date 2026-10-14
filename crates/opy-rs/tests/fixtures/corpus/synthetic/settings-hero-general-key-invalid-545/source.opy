settings {
    "main": {
        "description": "opy-rs#545 general keys under a hero team"
    },
    "gamemodes": {
        "skirmish": {}
    },
    "heroes": {
        "allTeams": {
            "health%": 100,
            "passiveHealthRegen": false
        },
        "team1": {
            "damageReceived%": 50
        },
        "team2": {
            "enableHeadshotsOnly": true
        }
    }
}

rule "setup":
    @Event global
    pass
