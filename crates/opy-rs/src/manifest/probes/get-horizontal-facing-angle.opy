globalvar horizontalAngle

rule "r":
    @Event eachPlayer
    horizontalAngle = eventPlayer.getHorizontalFacingAngle()
