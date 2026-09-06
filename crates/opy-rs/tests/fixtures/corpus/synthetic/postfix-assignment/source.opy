globalvar counter = 0
globalvar indexed = [0]
playervar points = 0

rule "postfix assignments":
    @Event eachPlayer
    counter++
    counter--
    indexed[0]++
    indexed[0]--
    eventPlayer.points++
    eventPlayer.points--
