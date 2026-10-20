settings {
    "main": {
        "description": "roadhog secondaryFire key without a mapped ability"
    },
    "gamemodes": {
        "skirmish": {}
    },
    "heroes": {
        "allTeams": {
            "roadhog": {
                "secondaryFireCooldown%": 50
            }
        }
    }
}

rule "setup":
    @Event global
    pass
