settings {
    "main": {
        "description": "opy-rs#565 roadhog secondaryFire/ability2 reslot"
    },
    "gamemodes": {
        "skirmish": {}
    },
    "heroes": {
        "allTeams": {
            "roadhog": {
                "secondaryFireCooldown%": 50,
                "ability2Cooldown%": 50,
                "enableAbility2": false
            }
        }
    }
}

rule "setup":
    @Event global
    pass
