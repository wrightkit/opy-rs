globalvar g

rule "numeric literal in a Team position":
    @Event eachPlayer
    @Team 2
    g = getLivingPlayers(2)
    setTeamScore(2, 5)
