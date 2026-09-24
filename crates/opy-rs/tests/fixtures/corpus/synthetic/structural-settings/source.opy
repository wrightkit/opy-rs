settings {
    "gamemodes": {"general": {"heroLimit": "off"}},
    "heroes": {
        "team2": {
            "bastion": {"health%": 500},
            "general": {"damageReceived%": 25, "health%": 300}
        },
        "allTeams": {
            "ana": {"health%": 50},
            "general": {"abilityCooldown%": 40}
        }
    }
}

globalvar g

rule "settings":
    @Event global
    g = 1
