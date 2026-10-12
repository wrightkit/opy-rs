playervar target

rule "x":
    @Event eachPlayer
    target = eventPlayer
    eventPlayer.setMoveSpeed(150)
