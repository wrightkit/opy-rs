rule "horizontal facing angle":
    @Event eachPlayer
    @Condition eventPlayer.getHorizontalFacingAngle() == 0
    disableInspector()
