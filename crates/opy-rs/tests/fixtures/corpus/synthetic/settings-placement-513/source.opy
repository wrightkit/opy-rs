playervar score

settings {
    "main": {
        "modeName": "Placement repro"
    },
    "gamemodes": {
        "ffa": {
            "scoreToWin": 7
        }
    }
}

rule "Count":
    @Event playerEarnedElimination
    eventPlayer.score += 1
