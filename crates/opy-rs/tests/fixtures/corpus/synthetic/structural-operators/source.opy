#!optimizeStrict

globalvar g
globalvar h
playervar p

rule "operators":
    @Event eachPlayer
    @Condition eventPlayer.isDummy() != false
    @Condition eventPlayer.isAlive() == true
    @Condition not eventPlayer.p
    @Condition eventPlayer.p != true
    @Condition eventPlayer.p
    @Condition h > 0
    g = -5
    g = -(-h)
    g = 2 + 3
    g = -5 + h
    g = 3 > 2
    g = not true
    g = true and false
    g = (h > 1) and true
    g = 1 if 1 == 1 else 2
    g = h * 0
    g = h + 0
    g = -h
    g = not (h == 1)
    g = eventPlayer.isAlive() != false
    g = "am" == "**"
