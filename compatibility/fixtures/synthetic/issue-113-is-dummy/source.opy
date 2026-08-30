rule "isDummy residual":
    @Event eachPlayer
    @Condition eventPlayer.isDummy()
    disableInspector()
