settings {
    "main": {
        "modeName": w"Free for all",
        "description": w"a{0}b".format("x")
    },
    "gamemodes": {
        "ffa": {
            "enabledMaps": [
                "workshopIsland"
            ]
        }
    }
}

globalvar value

rule "issue 530 string-modifier settings":
    @Event global
    value = w"a{0}b".format(getMatchTime())
