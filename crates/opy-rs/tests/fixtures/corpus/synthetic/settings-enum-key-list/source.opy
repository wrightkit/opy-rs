settings {
    "main": {
        "description": "enum-keyed settings value list"
    },
    "gamemodes": {
        "ffa": {
            "enabled": true,
            "mapRotation": ["a"]
        }
    }
}

rule "enum key settings":
    @Event global
    pass
