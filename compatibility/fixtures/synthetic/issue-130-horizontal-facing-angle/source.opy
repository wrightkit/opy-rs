globalvar horizontalAngle

rule "horizontal facing angle":
    @Event eachPlayer
    horizontalAngle = eventPlayer.getHorizontalFacingAngle()
