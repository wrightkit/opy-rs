settings {
    "main": {
        "description": "M11 bounded settings fixture"
    },
    "gamemodes": {
        "skirmish": {
            "enabledMaps": [
                "workshopIsland"
            ]
        },
        "general": {
            "heroLimit": "off",
            "respawnTime%": 30,
            "enableHeroSwitching": true
        }
    },
    "heroes": {
        "allTeams": {
            "mei": {
                "health%": 200
            }
        }
    }
}

rule "setup":
    @Event global
    disableInspector()
