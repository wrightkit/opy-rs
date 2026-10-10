playervar score

rule "count":
    @Event playerEarnedElimination
    eventPlayer.score += 1
    settings {
        "main": {"modeName": "Nested"},
        "gamemodes": {"ffa": {"scoreToWin": 7}}
    }
    eventPlayer.score += 1
