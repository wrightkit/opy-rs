globalvar g
rule "r":
    @Event eachPlayer
    getPlayersInRadius(eventPlayer.getPosition(), 10).setStatusEffect(eventPlayer, 30)
