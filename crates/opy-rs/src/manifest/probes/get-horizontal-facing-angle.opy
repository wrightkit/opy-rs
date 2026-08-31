rule "r":
    @Event eachPlayer
    @Condition eventPlayer.getHorizontalFacingAngle() == 0
    disableInspector()
