globalvar g

rule "player set lengths":
    @Event eachPlayer
    g = len(getLivingPlayers(Team.1))
    g = len(getDeadPlayers(Team.1))
    g = len(getPlayers(Team.1))
    g = len(getPlayersOnObjective(Team.1))
    g = len(getPlayersOnHero(Hero.ANA, Team.1))
    g = len(getAllPlayers())
    g = len(getPlayersNotOnObjective(Team.1))
    g = len(g)

#!optimizeForSize

rule "player set lengths sized":
    @Event eachPlayer
    g = len(getLivingPlayers(Team.1))
    g = len(getPlayersOnHero(Hero.ANA, Team.1))

#!disableOptimizations

rule "player set lengths unoptimized":
    @Event eachPlayer
    g = len(getLivingPlayers(Team.1))
    g = len(getPlayersOnHero(Hero.ANA, Team.1))
    g = len(g)
