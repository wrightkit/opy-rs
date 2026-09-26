rule "startHoT positional signature":
    @Event eachPlayer
    eventPlayer.startHoT(null, 2, 3)

rule "startHoT keyword signature":
    @Event eachPlayer
    eventPlayer.startHoT(healer=null, duration=2, healingPerSecond=3)
