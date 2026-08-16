globalvar g
rule "r":
    @Event eachPlayer
    @Condition len(getPlayersInRadius(eventPlayer.getPosition(), 10)) > 0
    @Condition len(getPlayersInRadius(eventPlayer.getPosition(), 10, Team.ALL, LosCheck.OFF)) > 0
    disableInspector()
