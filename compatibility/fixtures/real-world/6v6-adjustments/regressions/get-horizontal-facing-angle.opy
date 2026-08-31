globalvar horizontalAngle

rule "horizontal facing angle residual":
    @Event eachPlayer
    horizontalAngle = eventPlayer.getHorizontalFacingAngle()
